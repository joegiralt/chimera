port module Gallery exposing (main)

{-| Every component of the UI in every state, DAY and NIGHT side by side,
and the JITTER probe: it steps every control through its range and reports
the largest shift of any box, which must stay 0.00 px.
-}

import Browser
import Dict exposing (Dict)
import Html exposing (Html, button, div, h1, span, text)
import Html.Attributes exposing (class, classList)
import Html.Events exposing (onClick)
import Json.Decode as Decode
import Protocol exposing (Addr(..), OptIndex, addrKey)
import Ui.Control as Control exposing (Chosen, Intent(..), Option, Reading, Sync(..), Target(..))
import Ui.Fader as Fader
import Ui.Knob as Knob
import Ui.Menu as Menu
import Ui.Module as Module exposing (Fold(..), Jack(..))
import Ui.Pad as Pad exposing (Pad(..))
import Ui.PadStrip as PadStrip
import Ui.Page as Page exposing (Current(..), Effect(..), Link(..))
import Ui.PatchBay as PatchBay
import Ui.Routes as Routes exposing (Dest(..), Routes, Source(..))
import Ui.Screen as Screen exposing (Glyph(..))
import Ui.Selector as Selector
import Ui.Toggle as Toggle
import Ui.Tokens as Tokens exposing (Os(..), Theme(..))
import Ui.WordRow as WordRow
import Unit exposing (Engine(..))


port storeTheme : String -> Cmd msg


{-| Ask the page's helper to measure every box once this sample is drawn.
-}
port probe : Int -> Cmd msg


port probed : (Decode.Value -> msg) -> Sub msg


main : Program Decode.Value Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , view = view
        , subscriptions = subscriptions
        }



-- WHAT THE GALLERY HOLDS


{-| How a value prints, the box's own formats.
-}
type Format
    = Hertz
    | Millis
    | Signed
    | Semis
    | Plain


type Entry
    = Cont { norm : Float, format : Format, sync : Sync, override : Maybe String }
    | Pick { options : List OptIndex, current : OptIndex, sync : Sync }


type Layout
    = Both
    | One


type Which
    = DayColumn
    | NightColumn
    | OneColumn


type alias Column =
    { page : Page.State, menu : Menu.State }


type Bay
    = OneCable
    | ManyCables


type Jitter
    = Idle
    | Running { k : Int, shift : Float, boxes : Int, saved : Saved }
    | Done { shift : Float, boxes : Int, samples : Int }


type alias Saved =
    { values : Dict String Entry, many : Routes, one : Routes, pad : PadStrip.Tab }


type alias Model =
    { values : Dict String Entry
    , drag : Control.Drag
    , layout : Layout
    , day : Column
    , night : Column
    , one : Column
    , bayOne : PatchBay.Bay
    , bayMany : PatchBay.Bay
    , routesOne : Routes
    , routesMany : Routes
    , pad : PadStrip.Tab
    , jitter : Jitter
    }


type Msg
    = Hand Control.Msg
    | MenuMsg Which Menu.Msg
    | PageMsg Which Page.Msg
    | BayMsg Bay PatchBay.Msg
    | PadMsg PadStrip.Msg
    | SetLayout Layout
    | StartJitter
    | Probed Decode.Value


init : Decode.Value -> ( Model, Cmd Msg )
init flags =
    let
        preference : Tokens.Preference
        preference =
            Decode.decodeValue
                (Decode.map2 (\s d -> { stored = s, os = os d })
                    (Decode.field "stored" (Decode.nullable Decode.string))
                    (Decode.field "dark" Decode.bool)
                )
                flags
                |> Result.withDefault { stored = Nothing, os = OsLight }

        fresh : Theme -> Column
        fresh t =
            { page =
                Page.init t
                    |> Page.update (Page.Folding (Module.SetFold [ Module.idOf (Module.new "FOLDED") ] Folded))
                    |> Tuple.first
            , menu = Menu.shut
            }
    in
    ( { values = Dict.fromList (List.map (\( a, e ) -> ( addrKey a, e )) entries)
      , drag = Control.idle
      , layout = Both
      , day = fresh Day
      , night = fresh Night
      , one = fresh (Tokens.fromPreference preference)
      , bayOne = PatchBay.init
      , bayMany = PatchBay.init
      , routesOne = Routes.add (cable "ENV1" "FREQ" 30) blankRoutes
      , routesMany = List.foldl Routes.add blankRoutes [ cable "ENV1" "FREQ" 0, cable "VELO" "OUT DECAY" 30, cable "NOTE" "FREQ" -45, cable "LFO2" "Q" 80, cable "ENV3" "VCA" -100 ]
      , pad = PadStrip.PadTab Pad9
      , jitter = Idle
      }
    , Cmd.none
    )


os : Bool -> Os
os dark =
    if dark then
        OsDark

    else
        OsLight


blankRoutes : Routes
blankRoutes =
    Routes.new
        (List.map Source [ "ENV1", "ENV2", "ENV3", "LFO1", "LFO2", "LFO3", "VELO", "NOTE", "WHEEL" ])
        (List.map Dest [ "FREQ", "Q", "VCA", "FX", "OUT DECAY", "PIT PITCH" ])


cable : String -> String -> Int -> Routes.Cable
cable s d a =
    { from = Source s, to = Dest d, amount = Routes.amount a, addr = Addr ("MOD." ++ s ++ "." ++ d) }



-- THE CATALOGUE'S CONTROLS


{-| The first `n` option numbers.
-}
indices : Int -> List OptIndex
indices n =
    List.filterMap Protocol.optIndex (List.range 0 (n - 1))


options : List String -> List Option
options names =
    List.map2 Tuple.pair (indices (List.length names)) names


cont : Float -> Format -> Entry
cont n f =
    Cont { norm = n, format = f, sync = Confirmed, override = Nothing }


pick : Int -> Int -> Entry
pick n i =
    case List.drop i (indices n) of
        current :: _ ->
            Pick { options = indices n, current = current, sync = Confirmed }

        [] ->
            cont 0 Plain


steal : List Option
steal =
    options [ "CUT", "GLIDE" ]


speed : List Option
speed =
    options [ "FAST", "MED", "SLOW" ]


wave : List Option
wave =
    options [ "SINE", "TRI", "SAW", "SQUARE" ]


modes : List Option
modes =
    options [ "LP4", "LP2", "LP1", "BP2", "BP4", "HP4", "NOTCH1", "NOTCH2" ]


algs : List Option
algs =
    options ([ "T1", "T2" ] ++ List.map (\i -> "A" ++ String.fromInt i) (List.range 1 8) ++ [ "A32 LONG" ])


shapes : List Option
shapes =
    options [ "ADSR", "FUNCTION", "CLASSIC", "WAVE", "OP WAVE", "CLEAN", "SAW", "PULSE", "NOISE" ]


accents : List Option
accents =
    options [ "TEAL", "AMBER", "RED", "PUNCH", "WHITE" ]


entries : List ( Addr, Entry )
entries =
    [ ( Addr "K.CUTOFF", cont 0.62 Hertz )
    , ( Addr "K.HERO", cont 0.45 Hertz )
    , ( Addr "K.TRNSP", cont 0.5 Semis )
    , ( Addr "K.NARROW", cont 0.3 Plain )
    , ( Addr "K.PEND", cont 0.7 Millis )
    , ( Addr "F.LEVEL", cont 0.98 Plain )
    , ( Addr "F.HERO", cont 0.54 Plain )
    , ( Addr "F.BIP", cont 0.2 Signed )
    , ( Addr "F.ATTACK", cont 0.1 Millis )
    , ( Addr "T.STEAL0", pick 2 0 )
    , ( Addr "T.STEAL1", pick 2 1 )
    , ( Addr "T.SPEED0", pick 3 0 )
    , ( Addr "T.SPEED1", pick 3 1 )
    , ( Addr "T.SPEED2", pick 3 2 )
    , ( Addr "S.WAVE0", pick 4 0 )
    , ( Addr "S.WAVE1", pick 4 1 )
    , ( Addr "S.WAVE2", pick 4 2 )
    , ( Addr "S.WAVE3", pick 4 3 )
    , ( Addr "S.MODE", pick 8 6 )
    , ( Addr "M.ALG", pick 11 0 )
    , ( Addr "M.SHAPE", pick 9 1 )
    , ( Addr "M.WAVE", pick 11 10 )
    , ( Addr "W.ACCENT", pick 5 3 )
    ]
        |> List.map
            (\( a, e ) ->
                if a == Addr "K.PEND" then
                    ( a, pend e )

                else
                    ( a, e )
            )


pend : Entry -> Entry
pend e =
    case e of
        Cont c ->
            Cont { c | sync = Pending }

        Pick p ->
            Pick { p | sync = Pending }


format : Format -> Float -> String
format f t =
    case f of
        Hertz ->
            hertz (20 * (1000 ^ t))

        Millis ->
            millis (0.1 * (100000 ^ t))

        Signed ->
            signed (round ((t - 0.5) * 200))

        Semis ->
            signed (round ((t - 0.5) * 48)) ++ " st"

        Plain ->
            String.fromInt (round (t * 127))


hertz : Float -> String
hertz x =
    if x >= 1000 then
        decimals 2 (x / 1000) ++ " kHz"

    else if x < 10 then
        decimals 2 x ++ " Hz"

    else
        String.fromInt (round x) ++ " Hz"


millis : Float -> String
millis x =
    if x >= 1000 then
        decimals 2 (x / 1000) ++ " s"

    else if x >= 10 then
        String.fromInt (round x) ++ " ms"

    else
        decimals 1 x ++ " ms"


decimals : Int -> Float -> String
decimals n x =
    let
        scaled : Int
        scaled =
            round (x * toFloat (10 ^ n))

        whole : String
        whole =
            String.fromInt (scaled // (10 ^ n))

        frac : String
        frac =
            String.padLeft n '0' (String.fromInt (modBy (10 ^ n) scaled))
    in
    whole ++ "." ++ frac


signed : Int -> String
signed v =
    if v > 0 then
        "+" ++ String.fromInt v

    else
        String.fromInt v


readingOf : Model -> String -> Reading
readingOf m key =
    case Dict.get key m.values of
        Just (Cont c) ->
            let
                r : Reading
                r =
                    Control.reading c.norm (Maybe.withDefault (format c.format c.norm) c.override)
            in
            case c.sync of
                Pending ->
                    Control.pending r

                Confirmed ->
                    r

        Just (Pick _) ->
            Control.reading 0 "--"

        Nothing ->
            Control.reading 0 "--"


chosenOf : Model -> String -> Maybe Chosen
chosenOf m key =
    case Dict.get key m.values of
        Just (Pick p) ->
            Just
                (case p.sync of
                    Pending ->
                        Control.pendingChoice (Control.chosen p.current)

                    Confirmed ->
                        Control.chosen p.current
                )

        Just (Cont _) ->
            Nothing

        Nothing ->
            Nothing



-- UPDATE


lookup : Model -> Addr -> Maybe Target
lookup m a =
    case List.filter (\c -> c.addr == a) (Routes.cables m.routesOne ++ Routes.cables m.routesMany) of
        c :: _ ->
            Just (Continuous { norm = Routes.amountNorm c.amount, wheel = 0.005 })

        [] ->
            case Dict.get (addrKey a) m.values of
                Just (Cont c) ->
                    Just (Continuous { norm = c.norm, wheel = 1 / 64 })

                Just (Pick p) ->
                    Just (Discrete { options = p.options, current = p.current })

                Nothing ->
                    Nothing


apply : Intent -> Model -> Model
apply intent m =
    case intent of
        SetNorm a t ->
            { m
                | values = Dict.update (addrKey a) (Maybe.map (setNorm t)) m.values
                , routesOne = Routes.setAmount a (Routes.amountFromNorm t) m.routesOne
                , routesMany = Routes.setAmount a (Routes.amountFromNorm t) m.routesMany
            }

        SetOption a i ->
            { m | values = Dict.update (addrKey a) (Maybe.map (setOption i)) m.values }

        Restore a ->
            apply (SetNorm a 0.5) m


setNorm : Float -> Entry -> Entry
setNorm t e =
    case e of
        Cont c ->
            Cont { c | norm = t }

        Pick _ ->
            e


setOption : OptIndex -> Entry -> Entry
setOption i e =
    case e of
        Pick p ->
            Pick { p | current = i }

        Cont _ ->
            e


hand : Control.Msg -> Model -> Model
hand msg m =
    let
        ( drag, intents ) =
            Control.update (lookup m) msg m.drag
    in
    List.foldl apply { m | drag = drag } intents


column : Which -> Model -> Column
column w m =
    case w of
        DayColumn ->
            m.day

        NightColumn ->
            m.night

        OneColumn ->
            m.one


setColumn : Which -> Column -> Model -> Model
setColumn w c m =
    case w of
        DayColumn ->
            { m | day = c }

        NightColumn ->
            { m | night = c }

        OneColumn ->
            { m | one = c }


update : Msg -> Model -> ( Model, Cmd Msg )
update msg m =
    case msg of
        Hand h ->
            ( hand h m, Cmd.none )

        MenuMsg w mm ->
            let
                ( menu, picked ) =
                    Menu.update mm

                c : Column
                c =
                    column w m
            in
            ( Maybe.map (\p -> hand p) picked |> Maybe.withDefault identity |> (\f -> f (setColumn w { c | menu = menu } m)), Cmd.none )

        PageMsg w pm ->
            let
                c : Column
                c =
                    column w m

                ( page, effect ) =
                    Page.update pm c.page
            in
            ( setColumn w { c | page = page } m
            , case effect of
                StoreTheme t ->
                    storeTheme (Tokens.name t)

                NoEffect ->
                    Cmd.none
            )

        BayMsg which bm ->
            let
                ( bay, effects ) =
                    PatchBay.update bm (bayOf which m)

                stored : Model
                stored =
                    case which of
                        OneCable ->
                            { m | bayOne = bay }

                        ManyCables ->
                            { m | bayMany = bay }
            in
            ( List.foldl (bayEffect which) stored effects, Cmd.none )

        PadMsg (PadStrip.Select t) ->
            ( { m | pad = t }, Cmd.none )

        SetLayout l ->
            ( { m | layout = l }, Cmd.none )

        StartJitter ->
            ( { m | jitter = Running { k = 0, shift = 0, boxes = 0, saved = { values = m.values, many = m.routesMany, one = m.routesOne, pad = m.pad } } }, probe 0 )

        Probed v ->
            probedSample v m


bayOf : Bay -> Model -> PatchBay.Bay
bayOf which m =
    case which of
        OneCable ->
            m.bayOne

        ManyCables ->
            m.bayMany


bayEffect : Bay -> PatchBay.Effect -> Model -> Model
bayEffect which effect m =
    let
        onRoutes : (Routes -> Routes) -> Model
        onRoutes f =
            case which of
                OneCable ->
                    { m | routesOne = f m.routesOne }

                ManyCables ->
                    { m | routesMany = f m.routesMany }
    in
    case effect of
        PatchBay.Patched (Source s) (Dest d) ->
            onRoutes (Routes.add (cable s d 30))

        PatchBay.Unpatched s d ->
            onRoutes (Routes.remove s d)

        PatchBay.Handed h ->
            hand h m



-- JITTER


samples : Int
samples =
    32


{-| Sample `k` of every control: continuous values across their travel
(sample 1 the shortest text, 2 the longest), every option of every choice,
every cable amount, pending on odd samples, every pad.
-}
atSample : Int -> Saved -> Model -> Model
atSample k saved m =
    let
        t : Float
        t =
            toFloat k / toFloat samples

        sync : Sync
        sync =
            if modBy 2 k == 1 then
                Pending

            else
                Confirmed

        override : Maybe String
        override =
            if k == 1 then
                Just ""

            else if k == 2 then
                Just "20000.00 kHz and then some"

            else
                Nothing

        step : Entry -> Entry
        step e =
            case e of
                Cont c ->
                    Cont { c | norm = t, sync = sync, override = override }

                Pick p ->
                    Pick { p | current = List.drop (modBy (max 1 (List.length p.options)) k) p.options |> List.head |> Maybe.withDefault p.current, sync = sync }

        amounts : Routes -> Routes
        amounts r =
            List.foldl (\c acc -> Routes.setAmount c.addr (Routes.amountFromNorm t) acc) r (Routes.cables r)
    in
    { m
        | values = Dict.map (\_ e -> step e) saved.values
        , routesOne = amounts saved.one
        , routesMany = amounts saved.many
        , pad = List.drop (modBy 13 k) (PadStrip.Kit :: List.map PadStrip.PadTab Pad.all) |> List.head |> Maybe.withDefault saved.pad
    }


probedSample : Decode.Value -> Model -> ( Model, Cmd Msg )
probedSample v m =
    case m.jitter of
        Running r ->
            let
                measured : { boxes : Int, shift : Float }
                measured =
                    Decode.decodeValue (Decode.map2 (\b s -> { boxes = b, shift = s }) (Decode.field "boxes" Decode.int) (Decode.field "shift" Decode.float)) v
                        |> Result.withDefault { boxes = 0, shift = -1 }
            in
            if r.k >= samples then
                ( { m
                    | jitter = Done { shift = measured.shift, boxes = measured.boxes, samples = samples }
                    , values = r.saved.values
                    , routesOne = r.saved.one
                    , routesMany = r.saved.many
                    , pad = r.saved.pad
                  }
                , Cmd.none
                )

            else
                ( atSample (r.k + 1) r.saved { m | jitter = Running { r | k = r.k + 1, shift = measured.shift, boxes = measured.boxes } }
                , probe (r.k + 1)
                )

        Idle ->
            ( m, Cmd.none )

        Done _ ->
            ( m, Cmd.none )


jitterText : Model -> String
jitterText m =
    case m.jitter of
        Idle ->
            "JITTER steps every control through its range"

        Running r ->
            "JITTER sample " ++ String.fromInt r.k ++ " of " ++ String.fromInt samples

        Done d ->
            "JITTER controls="
                ++ String.fromInt (Dict.size m.values + List.length (Routes.cables m.routesOne ++ Routes.cables m.routesMany) + 1)
                ++ " samples="
                ++ String.fromInt d.samples
                ++ " boxes="
                ++ String.fromInt d.boxes
                ++ (if d.shift < 0 then
                        " STRUCTURAL CHANGE"

                    else
                        " shift=" ++ decimals 2 d.shift ++ " px"
                   )



-- VIEW


subscriptions : Model -> Sub Msg
subscriptions m =
    Sub.batch
        [ Sub.map Hand (Control.subscriptions m.drag)
        , Sub.map (BayMsg OneCable) (PatchBay.subscriptions m.bayOne)
        , Sub.map (BayMsg ManyCables) (PatchBay.subscriptions m.bayMany)
        , probed Probed
        ]


view : Model -> Html Msg
view m =
    div [ class "gallery" ]
        [ div [ class "ghead" ]
            [ h1 [] [ text "CHIMERA UI · GALLERY" ]
            , div [ class "words" ]
                [ button [ classList [ ( "on", m.layout == Both ) ], onClick (SetLayout Both) ] [ text "DAY | NIGHT" ]
                , button [ classList [ ( "on", m.layout == One ) ], onClick (SetLayout One) ] [ text "ONE PAGE" ]
                ]
            , button [ class "jitter", onClick StartJitter ] [ text "JITTER" ]
            , span [ class "jres" ] [ text (jitterText m) ]
            ]
        , case m.layout of
            Both ->
                div [ class "gcols" ] [ pageOf DayColumn m, pageOf NightColumn m ]

            One ->
                pageOf OneColumn m
        ]


pageOf : Which -> Model -> Html Msg
pageOf w m =
    let
        c : Column
        c =
            column w m
    in
    Page.view (PageMsg w)
        c.page
        (Page.new "GALLERY"
            |> Page.meta "EVERY COMPONENT · EVERY STATE"
            |> Page.parts
                [ { kicker = "P2 · ALGO", name = "GLASS BELLS", current = Here, go = SetLayout m.layout }
                , { kicker = "P3 · DRUM", name = "DRUM INIT", current = Elsewhere, go = SetLayout m.layout }
                ]
            |> Page.units [ { kicker = "UNIT", name = "MIXER", current = Elsewhere, go = SetLayout m.layout } ]
            |> Page.link (Linked "PREENFM3 · 0.4.0 · LIVE")
            |> Page.strip (Html.map PadMsg (PadStrip.view (PadStrip.new "DRUM INIT" padName) m.pad))
            |> Page.modules (catalogue w c.menu m)
        )


padName : Pad -> String
padName p =
    case p of
        Pad1 ->
            "KICK"

        Pad2 ->
            "SNARE"

        Pad3 ->
            "IMPULSE"

        Pad4 ->
            "CLAP"

        Pad5 ->
            "TOM"

        Pad6 ->
            "TOM"

        Pad7 ->
            "TOM"

        Pad8 ->
            "HAT"

        Pad9 ->
            "HAT"

        Pad10 ->
            "CYMBAL"

        Pad11 ->
            "COWBELL"

        Pad12 ->
            "SHAKER"


catalogue : Which -> Menu.State -> Model -> List (Module.Config Msg)
catalogue w menu m =
    let
        r : String -> Reading
        r =
            readingOf m

        ch : String -> Maybe Chosen
        ch =
            chosenOf m

        hands : Html Control.Msg -> Html Msg
        hands =
            Html.map Hand

        menuView : Maybe Menu.Config -> String -> Html Msg
        menuView config key =
            case ( config, ch key ) of
                ( Just mc, Just c ) ->
                    Html.map (MenuMsg w) (Menu.view mc menu c)

                ( Just _, Nothing ) ->
                    text ""

                ( Nothing, _ ) ->
                    text ""

        toggle : Maybe Toggle.Config -> String -> Html Msg
        toggle config key =
            Maybe.map2 (\tc c -> hands (Toggle.view tc c)) config (ch key) |> Maybe.withDefault (text "")

        selector : String -> List Option -> String -> Html Msg
        selector lab opts key =
            Maybe.map2 (\sc c -> hands (Selector.view (Selector.label lab sc) c)) (Selector.fromOptions (Addr key) opts) (ch key)
                |> Maybe.withDefault (text "")

        knob : String -> (Knob.Config -> Knob.Config) -> String -> Html Msg
        knob lab f key =
            hands (Knob.view (Knob.new (Addr key) |> Knob.label lab |> f) (r key))

        fader : String -> (Fader.Config -> Fader.Config) -> String -> Html Msg
        fader lab f key =
            hands (Fader.view (Fader.new (Addr key) |> Fader.label lab |> f) (r key))

        screen : Glyph -> String -> Html Msg
        screen g cap =
            Screen.view (Screen.new Algo g |> Screen.caption cap)
    in
    [ Module.new "KNOB"
        |> Module.note "PLAIN · HERO · BIPOLAR · NARROW · PENDING"
        |> Module.chip "CONTROLS"
        |> Module.summary [ ( "CUTOFF", Control.text (r "K.CUTOFF") ) ]
        |> Module.body
            [ Module.panel
                [ Module.group "" [ knob "CUTOFF" identity "K.CUTOFF", knob "TRANSPOSE" Knob.bipolar "K.TRNSP", knob "FINE" (Knob.chars 3) "K.NARROW", knob "DECAY" identity "K.PEND" ]
                , Module.group "HERO" [ knob "CUTOFF" Knob.hero "K.HERO" ]
                ]
            ]
    , Module.new "FADER"
        |> Module.note "PLAIN · HERO · BIPOLAR"
        |> Module.chip "CONTROLS"
        |> Module.jacks [ In "LEVEL", Out "ENV" ]
        |> Module.body
            [ Module.panel
                [ Module.group "OUTPUT" [ fader "LEVEL" Fader.hero "F.HERO", fader "VELO" identity "F.LEVEL" ]
                , Module.group "ENVELOPE" [ fader "ATTACK" identity "F.ATTACK", fader "AMOUNT" Fader.bipolar "F.BIP" ]
                ]
            ]
    , Module.new "TOGGLE"
        |> Module.note "EVERY POSITION"
        |> Module.chip "CHOICES"
        |> Module.body
            [ Module.panel
                [ Module.group "TWO" (List.map (\i -> toggle (Toggle.fromOptions (Addr ("T.STEAL" ++ String.fromInt i)) steal |> Maybe.map (Toggle.label "STEAL")) ("T.STEAL" ++ String.fromInt i)) [ 0, 1 ])
                , Module.group "THREE" (List.map (\i -> toggle (Toggle.fromOptions (Addr ("T.SPEED" ++ String.fromInt i)) speed |> Maybe.map (Toggle.label "SPEED")) ("T.SPEED" ++ String.fromInt i)) [ 0, 1, 2 ])
                ]
            ]
    , Module.new "SELECTOR"
        |> Module.note "FOUR TO EIGHT"
        |> Module.chip "CHOICES"
        |> Module.body
            [ Module.panel
                [ Module.group "FOUR" (List.map (\i -> selector "WAVE" wave ("S.WAVE" ++ String.fromInt i)) [ 0, 1, 2, 3 ])
                , Module.group "EIGHT" [ selector "MODE" modes "S.MODE" ]
                ]
            ]
    , Module.new "MENU"
        |> Module.note "MORE THAN EIGHT"
        |> Module.chip "CHOICES"
        |> Module.menus [ menuView (Control.options algs |> Maybe.map (Menu.new (Addr "M.WAVE")) |> Maybe.map (Menu.label "WAVE" >> Menu.inHeader)) "M.WAVE" ]
        |> Module.body
            [ Module.panel
                [ Module.group "" [ menuView (Control.options algs |> Maybe.map (Menu.new (Addr "M.ALG")) |> Maybe.map (Menu.label "ALG A")) "M.ALG" ]
                , Module.group "WITH GLYPHS" [ menuView (Control.options shapes |> Maybe.map (Menu.new (Addr "M.SHAPE")) |> Maybe.map (Menu.label "SHAPE" >> Menu.glyphs shapeGlyph)) "M.SHAPE" ]
                ]
            ]
    , Module.new "WORD ROW"
        |> Module.note "SETTINGS SHEET"
        |> Module.chip "CHOICES"
        |> Module.body [ Maybe.map2 (\opts c -> hands (WordRow.view (WordRow.new (Addr "W.ACCENT") opts |> WordRow.label "ACCENT") c)) (Control.options accents) (ch "W.ACCENT") |> Maybe.withDefault (text "") ]
    , Module.new "SCREENS"
        |> Module.note "THE UNIT'S GLYPHS"
        |> Module.chip "SCREENS"
        |> Module.body
            [ Module.panel
                [ Module.group "" [ screen AlgoChart "ALGORITHM", screen EnvAdsr "ENV", screen EnvFunction "FUNCTION", screen FilterClean "CLEAN" ]
                , Module.group "" [ screen LfoClassic "LFO", screen VaWave "WAVE", screen OpWave "OP" ]
                ]
            ]
    , Module.new "TALL NEIGHBOUR"
        |> Module.note "A ROW TAKES THE TALLER HEIGHT"
        |> Module.chip "SCREENS"
        |> Module.body [ Module.panel [ Module.group "" [ fader "A" identity "F.ATTACK", fader "D" identity "F.LEVEL" ] ] ]
    , Module.new "FOLDED"
        |> Module.note "FOLDS ITS ROW"
        |> Module.chip "FOLDING"
        |> Module.summary [ ( "LEVEL", Control.text (r "F.LEVEL") ), ( "", Control.text (r "K.CUTOFF") ) ]
        |> Module.body [ text "folded" ]
    , Module.new "ITS PAIR"
        |> Module.chip "FOLDING"
        |> Module.summary [ ( "MODE", "LP4" ) ]
        |> Module.body [ text "folds with its row" ]
    , Module.new "MOD"
        |> Module.note "ONE CABLE"
        |> Module.chip "MOD"
        |> Module.full
        |> Module.body [ Html.map (BayMsg OneCable) (PatchBay.view m.bayOne m.routesOne) ]
    , Module.new "MOD · MANY"
        |> Module.note "PATCH | MATRIX"
        |> Module.chip "MOD"
        |> Module.full
        |> Module.body [ Html.map (BayMsg ManyCables) (PatchBay.view m.bayMany m.routesMany) ]
    , Module.new "ALONE"
        |> Module.note "A LONE HALF BESIDE A BLANK"
        |> Module.chip "FOLDING"
        |> Module.body [ text "half a row" ]
    ]


shapeGlyph : OptIndex -> Maybe Screen.Config
shapeGlyph i =
    [ EnvAdsr, EnvFunction, LfoClassic, VaWave, OpWave, FilterClean ]
        |> List.map2 Tuple.pair (indices 6)
        |> List.filter (\( k, _ ) -> k == i)
        |> List.head
        |> Maybe.map (\( _, g ) -> Screen.new Algo g)
