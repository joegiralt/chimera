module Model exposing
    ( Effect(..)
    , Held
    , Model
    , Msg(..)
    , Reading
    , RouteReading
    , Seat
    , Stage
    , Via(..)
    , init
    , notice
    , refusal
    , seat
    , shown
    , status
    , step
    , subscriptions
    , takesEdits
    , update
    )

{-| Only the unit's state: what it last reported, and what it refused. The
editor never guesses a value: a control always shows the unit's stored
value, and an edit the unit would not take shows the unit's reason.

`step` is the pure core (messages in, effects out); `update` only runs them.

The schemas are the unit's own text (ADR 0289): on connect, and on an engine
the editor hasn't read, it asks `ed schema <S>` for its page count and hash,
and uses its cached copy of that hash (IndexedDB, `Cache`) or describes every
page and keeps them. It reads the followed engine's schema, the MIXER's and
the engine's matrix.

-}

import Addr exposing (Addr)
import Cache
import Dict exposing (Dict)
import Protocol exposing (AnswerLine(..), Cell, Command(..), Hash, Line(..), PageNo, RouteTo, SchemaId(..), Value, cellKey, encode, hashText, parse, schemaToken)
import Schema exposing (Schema)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine, Gen, Part)


{-| One value as the unit reported it: its text is the box's own.
-}
type alias Reading =
    { norm : Float, stored : Float, text : String }


{-| A route as the unit reported it: its amount and the box's text.
-}
type alias RouteReading =
    { amount : Int, text : String }


{-| Where the connect sequence is. One request is in flight at a time.
A schema is checked (`ed schema`), recalled from the cache, or described page
by page; then the next one the editor lacks, then following.
-}
type Stage
    = Greeting
    | Checking SchemaId (Maybe Reply)
    | Recalling SchemaId Reply
    | Describing SchemaId Reply PageNo
    | Following
    | Halted Why


{-| What `ed schema` said of a schema.
-}
type alias Reply =
    { pages : Int, hash : Hash }


{-| A schema read this connection, and the hash it was read under.
-}
type alias Held =
    { hash : Hash, schema : Schema }


{-| What `step` asks of the world: a line to the unit, or the schema cache.
-}
type Effect
    = Send Command
    | Recall String
    | Keep String (List String)


type Why
    = UpdateEditor
    | UpdateUnit
    | RefusedBy String
    | Silent
    | TooLarge


{-| Which Part the box was following when a hand took hold: a new `part`
line is a new seat, and a hand from the old one lets go.
-}
type Seat
    = Seat Int


type alias Model =
    { link : Maybe Link
    , via : Maybe Via
    , schema : Schema
    , mixer : Schema
    , matrix : Schema
    , routes : Dict String RouteReading
    , held : Dict String Held
    , incoming : Schema

    -- The schema being described, as the unit wrote it (last first), for the cache.
    , incomingLines : List String
    , values : Dict String Reading
    , refusals : Dict String String
    , whole : Bool
    , part : Maybe Part
    , engine : Maybe Engine
    , described : Maybe Engine
    , seat : Seat
    , variants : Dict String String
    , source : Maybe String
    , gen : Maybe Gen
    , stage : Stage
    , asked : Maybe Time.Posix
    , owed : Int
    , leftover : Bool
    , now : Time.Posix
    }


{-| What the user chose to connect to.
-}
type Via
    = ViaUnit
    | ViaSimulator


type Msg
    = LinkChanged Link
    | Line String
    | Tick Time.Posix
    | Edit Addr Value
    | Pick Addr Protocol.OptIndex
    | FollowPart Part
    | Connect Via
    | Recalled { key : String, lines : Maybe (List String) }
    | EditRoute Cell RouteTo


{-| The console protocol this editor speaks.
-}
supported : Int
supported =
    4


{-| A schema's cache key: the protocol, the schema and its hash.
-}
cacheKey : SchemaId -> Hash -> String
cacheKey s h =
    String.join "/" [ String.fromInt supported, schemaToken s, hashText h ]


{-| A request unanswered this long has timed out. While connecting, that
halts: a late `OK` could not be told from the next request's, so it would
skip a step. Following never halts. A `since` is asked once more with the last
number we did get, and no further until every answer owed (`owed`) has come;
the unit answers in order, so a late `OK` is only counted, and its `val` lines
are true, as the number only moves forward. An `OK` that leaves answers owed
restarts the clock (`leftover`): if the rest never come, the next timeout
counts them lost and asks afresh, as does a re-ask that is lost too. The unit
heals a lost answer on the next ask.
-}
answerTimeoutMs : Int
answerTimeoutMs =
    1000


pollMs : Float
pollMs =
    33


init : Model
init =
    { link = Nothing
    , via = Nothing
    , schema = Schema.empty
    , mixer = Schema.empty
    , matrix = Schema.empty
    , routes = Dict.empty
    , held = Dict.empty
    , incoming = Schema.empty
    , incomingLines = []
    , values = Dict.empty
    , refusals = Dict.empty
    , whole = False
    , part = Nothing
    , engine = Nothing
    , described = Nothing
    , seat = Seat 0
    , variants = Dict.empty
    , source = Nothing
    , gen = Nothing
    , stage = Greeting
    , asked = Nothing
    , owed = 0
    , leftover = False
    , now = Time.millisToPosix 0
    }


subscriptions : Sub Msg
subscriptions =
    Sub.batch
        [ Transport.lineIn Line
        , Transport.linkState (Transport.parseLink >> LinkChanged)
        , Cache.got Recalled
        , Time.every pollMs Tick
        ]


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    let
        ( m, effects ) =
            step msg model
    in
    ( m, Cmd.batch (opening msg :: List.map run effects) )


{-| The one thing besides effects a message asks of the world: a link.
-}
opening : Msg -> Cmd msg
opening msg =
    case msg of
        Connect ViaUnit ->
            Transport.connectSerial ()

        Connect ViaSimulator ->
            Transport.connectSocket "ws://127.0.0.1:7342"

        LinkChanged _ ->
            Cmd.none

        Line _ ->
            Cmd.none

        Tick _ ->
            Cmd.none

        Edit _ _ ->
            Cmd.none

        Pick _ _ ->
            Cmd.none

        FollowPart _ ->
            Cmd.none

        Recalled _ ->
            Cmd.none

        EditRoute _ _ ->
            Cmd.none


run : Effect -> Cmd msg
run e =
    case e of
        Send c ->
            encode c |> Maybe.map Transport.send |> Maybe.withDefault Cmd.none

        Recall key ->
            Cache.recall key

        Keep key lines ->
            Cache.keep { key = key, lines = lines }


{-| Once connected and following: the link and the Part and engine it follows.
-}
status : Model -> Maybe String
status m =
    if m.link == Just Open && m.stage == Following then
        Maybe.map2
            (\p e ->
                String.join " · "
                    [ if m.via == Just ViaSimulator then
                        "SIMULATOR"

                      else
                        "UNIT"
                    , "PART " ++ String.fromInt (Unit.partNumber p)
                    , Unit.engineName e
                    ]
            )
            m.part
            m.engine

    else
        Nothing


{-| What to tell the user instead of the editor, if anything.
-}
notice : Model -> Maybe String
notice m =
    case m.link of
        Just l ->
            if l /= Open then
                Transport.linkText l

            else
                stageNotice m

        Nothing ->
            Nothing


stageNotice : Model -> Maybe String
stageNotice m =
    case m.stage of
        Halted UpdateEditor ->
            Just "UPDATE THE EDITOR"

        Halted UpdateUnit ->
            Just "UPDATE THE UNIT"

        Halted (RefusedBy t) ->
            Just ("THE UNIT REFUSED: " ++ t)

        Halted Silent ->
            Just "THE UNIT STOPPED ANSWERING"

        Halted TooLarge ->
            Just "THE UNIT'S ANSWER IS TOO LARGE FOR ONE READ"

        Greeting ->
            Nothing

        Checking _ _ ->
            Nothing

        Recalling _ _ ->
            Nothing

        Describing _ _ _ ->
            Nothing

        Following ->
            Nothing


shown : Addr -> Model -> Maybe Reading
shown a m =
    Dict.get (Addr.key a) m.values


{-| The unit's reason for refusing the last edit of `a`, until it next
reports a value for it.
-}
refusal : Addr -> Model -> Maybe String
refusal a m =
    Dict.get (Addr.key a) m.refusals


seat : Model -> Seat
seat m =
    m.seat


{-| Edits go out only while following, and only against the schema of the
engine the box is on: during an engine change a shared address (FILTER.CUTOFF)
would mean the old engine's parameter.
-}
takesEdits : Model -> Bool
takesEdits m =
    m.stage == Following && m.engine /= Nothing && m.engine == m.described


step : Msg -> Model -> ( Model, List Effect )
step msg m =
    case msg of
        LinkChanged l ->
            -- Any link change starts over: nothing of the last link is shown.
            ( { init | link = Just l, via = m.via, now = m.now, seat = nextSeat m.seat }, [] )

        Tick t ->
            tick t m |> Tuple.mapSecond (List.map Send)

        Line raw ->
            line raw (parse raw) m

        Edit a v ->
            edit a v m |> Tuple.mapSecond (List.map Send)

        -- By its number: a name with a space (GATE EXT) is one word so.
        Pick a i ->
            case Schema.optionName m.variants a i (schemaOf a m) of
                Just _ ->
                    edit a (Protocol.Index i) m |> Tuple.mapSecond (List.map Send)

                Nothing ->
                    ( m, [] )

        -- The box moves; the mark follows when `since` says so.
        FollowPart p ->
            if m.stage == Following then
                ( m, [ Send (Follow p) ] )

            else
                ( m, [] )

        -- Opening the link is `update`'s (it asks the shim).
        Connect v ->
            ( { m | via = Just v }, [] )

        Recalled r ->
            ( recalled r m, [] )

        -- A route's amount or its removal: the unit glides it a turn's step a
        -- pass, and `since` says where it is.
        EditRoute c to ->
            if takesEdits m then
                ( m, [ Send (SetRoute c to) ] )

            else
                ( m, [] )


nextSeat : Seat -> Seat
nextSeat (Seat n) =
    Seat (n + 1)


{-| The schema that describes `a`: the followed engine's, else the mixer's.
-}
schemaOf : Addr -> Model -> Schema
schemaOf a m =
    case Schema.param a m.schema of
        Just _ ->
            m.schema

        Nothing ->
            m.mixer


{-| Some schema the editor holds describes `a`.
-}
isDescribed : Addr -> Model -> Bool
isDescribed a m =
    Schema.param a m.schema /= Nothing || isMixer a m


isMixer : Addr -> Model -> Bool
isMixer a m =
    Schema.param a m.mixer /= Nothing


tick : Time.Posix -> Model -> ( Model, List Command )
tick t m0 =
    let
        m : Model
        m =
            { m0 | now = t }

        ask : Command -> ( Model, List Command )
        ask c =
            ( { m | asked = Just t, owed = m.owed + 1, leftover = False }, [ c ] )
    in
    if m.link /= Just Open then
        ( m, [] )

    else
        let
            timedOut : Bool
            timedOut =
                m.asked |> Maybe.map (\at -> ms t - ms at >= answerTimeoutMs) |> Maybe.withDefault False

            silent : ( Model, List Command )
            silent =
                ( { m | stage = Halted Silent, asked = Nothing }, [] )
        in
        case m.stage of
            Halted _ ->
                ( m, [] )

            Following ->
                if m.owed == 0 then
                    ask (Since m.gen)

                else if timedOut && m.owed == 1 && not m.leftover then
                    ask (Since m.gen)

                else if timedOut then
                    -- Answers still owed after a re-ask: count them lost and ask afresh.
                    let
                        ( fresh, cs ) =
                            ask (Since m.gen)
                    in
                    ( { fresh | owed = 1 }, cs )

                else
                    ( m, [] )

            Greeting ->
                connecting timedOut silent (ask Hello) m

            Checking s _ ->
                connecting timedOut silent (ask (AskSchema s)) m

            -- The browser's store always answers, a miss or a failure as
            -- `Nothing`: no clock.
            Recalling _ _ ->
                ( m, [] )

            Describing s _ n ->
                connecting timedOut silent (ask (Describe s n)) m


connecting : Bool -> ( Model, List Command ) -> ( Model, List Command ) -> Model -> ( Model, List Command )
connecting timedOut silent go m =
    if timedOut then
        silent

    else if m.asked == Nothing then
        go

    else
        ( m, [] )


ms : Time.Posix -> Int
ms =
    Time.posixToMillis


edit : Addr -> Value -> Model -> ( Model, List Command )
edit a v m =
    let
        c : Command
        c =
            Set a v
    in
    if takesEdits m && isDescribed a m && encode c /= Nothing then
        ( m, [ c ] )

    else
        ( m, [] )


line : String -> Line -> Model -> ( Model, List Effect )
line raw l m =
    case m.stage of
        Halted _ ->
            ( m, [] )

        Greeting ->
            lineIn raw l m

        Checking _ _ ->
            lineIn raw l m

        Recalling _ _ ->
            lineIn raw l m

        Describing _ _ _ ->
            lineIn raw l m

        Following ->
            lineIn raw l m


lineIn : String -> Line -> Model -> ( Model, List Effect )
lineIn raw l m =
    case l of
        Answer a ->
            ( answer raw a m, [] )

        Ok_ done ->
            ok done False m

        OkMore done ->
            ok (Just done) True m

        Err_ why ->
            case m.stage of
                Following ->
                    -- A refused `since` stays owed; the timeout re-asks, then settles it as lost.
                    ( m, [] )

                Greeting ->
                    if String.startsWith "unknown command" why then
                        ( { m | stage = Halted UpdateUnit, asked = Nothing }, [] )

                    else
                        ( refused why m, [] )

                Checking _ _ ->
                    ( refused why m, [] )

                Recalling _ _ ->
                    ( refused why m, [] )

                Describing _ _ _ ->
                    ( refused why m, [] )

                Halted _ ->
                    ( m, [] )

        SetRefused a why ->
            ( { m | refusals = Dict.insert (Addr.key a) why m.refusals }, [] )

        Queued ->
            ( m, [] )

        -- Asking again gets the same: stop, and say so.
        SinceTooLarge ->
            ( { m | stage = Halted TooLarge, asked = Nothing }, [] )

        Unknown ->
            ( m, [] )


refused : String -> Model -> Model
refused why m =
    { m | stage = Halted (RefusedBy why), asked = Nothing }


answer : String -> AnswerLine -> Model -> Model
answer raw a m =
    case a of
        HelloIs h ->
            if h.protocol == supported then
                case Unit.engineFromName h.engine of
                    Just e ->
                        { m | part = Unit.partFromInt h.part, engine = Just e }

                    Nothing ->
                        { m | stage = Halted UpdateEditor }

            else
                { m
                    | stage =
                        Halted
                            (if h.protocol > supported then
                                UpdateEditor

                             else
                                UpdateUnit
                            )
                }

        SchemaIs r ->
            case m.stage of
                Checking s Nothing ->
                    if r.schema == s then
                        { m | stage = Checking s (Just { pages = r.pages, hash = r.hash }) }

                    else
                        m

                Checking _ (Just _) ->
                    m

                Greeting ->
                    m

                Recalling _ _ ->
                    m

                Describing _ _ _ ->
                    m

                Following ->
                    m

                Halted _ ->
                    m

        PartIs n name ->
            case Unit.engineFromName name of
                Just e ->
                    -- A new Part or engine: PART's section is whole; MIXER's stays.
                    -- An engine read this connection is shown at once.
                    adopt <|
                        mixerOnly
                            { m
                                | part = Unit.partFromInt n
                                , engine = Just e
                                , seat = nextSeat m.seat
                                , routes = Dict.empty
                                , refusals = Dict.empty
                                , whole = True
                                , source = Nothing
                            }

                Nothing ->
                    { m | stage = Halted UpdateEditor }

        MixerIs ->
            -- MIXER's section whole: what was held for it goes.
            { m
                | values = Dict.filter (\k _ -> not (isMixerKey k m)) m.values
                , variants = Dict.filter (\k _ -> not (Schema.hasPanel k m.mixer)) m.variants
            }

        Panel p ->
            if m.stage == Following then
                { m | variants = Dict.insert p.name p.variant m.variants }

            else
                describing raw a m

        Val v ->
            if not (isDescribed v.addr m) then
                -- The unit says nothing of it in `describe`: not the editor's.
                m

            else
                { m
                    | values = Dict.insert (Addr.key v.addr) { norm = v.norm, stored = v.stored, text = v.text } m.values
                    , refusals = Dict.remove (Addr.key v.addr) m.refusals
                }

        -- The followed Part's, in a diff: a whole answer (a new Part, or
        -- `since 0`) starts afresh, and an address the schema lacks is
        -- another engine's.
        Refused at why ->
            if m.whole || Schema.param at m.schema == Nothing then
                m

            else
                { m | refusals = Dict.insert (Addr.key at) why m.refusals }

        Source text ->
            if m.stage == Following then
                { m | source = Just text }

            else
                describing raw a m

        Page _ ->
            describing raw a m

        Param _ ->
            describing raw a m

        Opt _ ->
            describing raw a m

        Route _ ->
            describing raw a m

        Src _ ->
            describing raw a m

        Col _ ->
            describing raw a m

        AmountRange _ _ ->
            describing raw a m

        Dests _ ->
            describing raw a m

        RoutesIs ->
            { m | routes = Dict.empty }

        RouteIs r ->
            { m | routes = Dict.insert (cellKey r.cell) { amount = r.amount, text = r.text } m.routes }

        Unroute c ->
            { m | routes = Dict.remove (cellKey c) m.routes }


{-| Only the MIXER's values and panels kept: the Part's section starts afresh.
-}
mixerOnly : Model -> Model
mixerOnly m =
    { m
        | values = Dict.filter (\k _ -> isMixerKey k m) m.values
        , variants = Dict.filter (\k _ -> Schema.hasPanel k m.mixer) m.variants
    }


isMixerKey : String -> Model -> Bool
isMixerKey k m =
    Addr.fromWire k |> Maybe.map (\a -> isMixer a m) |> Maybe.withDefault False


{-| A schema read this connection, or none.
-}
heldSchema : SchemaId -> Model -> Maybe Schema
heldSchema s m =
    Dict.get (schemaToken s) m.held |> Maybe.map .schema


{-| A line of `ed describe`'s answer: into the schema being read, and kept
as the unit wrote it for the cache.
-}
describing : String -> AnswerLine -> Model -> Model
describing raw a m =
    case m.stage of
        Describing _ _ _ ->
            { m | incoming = Schema.add a m.incoming, incomingLines = raw :: m.incomingLines }

        Greeting ->
            m

        Checking _ _ ->
            m

        Recalling _ _ ->
            m

        Following ->
            m

        Halted _ ->
            m


{-| `OK` ends the request in flight. `more`: a `since` answer owes sections,
asked for on the next tick.
-}
ok : Maybe Int -> Bool -> Model -> ( Model, List Effect )
ok done more m0 =
    let
        m : Model
        m =
            { m0 | whole = False }
    in
    case m.stage of
        Greeting ->
            ( readNext { m | asked = Nothing, owed = 0 }, [] )

        Checking s (Just r) ->
            let
                m1 : Model
                m1 =
                    { m | asked = Nothing, owed = 0 }
            in
            case Dict.get (schemaToken s) m.held of
                Just h ->
                    if h.hash == r.hash then
                        ( readNext (install s h m1), [] )

                    else
                        ( { m1 | stage = Recalling s r }, [ Recall (cacheKey s r.hash) ] )

                Nothing ->
                    ( { m1 | stage = Recalling s r }, [ Recall (cacheKey s r.hash) ] )

        -- An `OK` with no `schema` line: not the unit this editor speaks to.
        Checking _ Nothing ->
            ( { m | stage = Halted UpdateEditor, asked = Nothing }, [] )

        Recalling _ _ ->
            ( m, [] )

        Describing s r n ->
            let
                m1 : Model
                m1 =
                    { m | asked = Nothing, owed = 0 }
            in
            if Protocol.pageNoToInt n + 1 < r.pages then
                ( { m1 | stage = Describing s r (Protocol.nextPage n) }, [] )

            else
                let
                    h : Held
                    h =
                        { hash = r.hash, schema = m.incoming }
                in
                ( readNext (install s h { m1 | incoming = Schema.empty, incomingLines = [] })
                , [ Keep (cacheKey s r.hash) (List.reverse m.incomingLines) ]
                )

        Following ->
            case done |> Maybe.andThen Unit.genFromInt of
                Just g ->
                    ( following g (max 0 (m.owed - 1)) more m, [] )

                Nothing ->
                    ( m, [] )

        Halted _ ->
            ( m, [] )


{-| A `since` answered. Once nothing is owed, a box on another engine has its
schemas read (held, cached or described), then is read from nothing; the old
page stays up meanwhile.
-}
following : Gen -> Int -> Bool -> Model -> Model
following g owed more m =
    if owed == 0 && not more && m.engine /= Nothing && m.engine /= m.described then
        readNext
            (adopt
                (mixerOnly
                    { m
                        | refusals = Dict.empty
                        , routes = Dict.empty
                        , gen = Nothing
                        , owed = 0
                        , leftover = False
                        , asked = Nothing
                    }
                )
            )

    else
        { m
            | gen = Just g
            , owed = owed
            , leftover = owed > 0
            , asked =
                if owed > 0 then
                    Just m.now

                else
                    Nothing
        }


{-| The followed engine's schemas, where this connection already read them.
-}
adopt : Model -> Model
adopt m =
    case m.engine |> Maybe.andThen (\e -> heldSchema (EngineSchema e) m |> Maybe.map (Tuple.pair e)) of
        Just ( e, sch ) ->
            { m | schema = sch, described = Just e, matrix = heldSchema (MatrixSchema e) m |> Maybe.withDefault Schema.empty }

        Nothing ->
            m


{-| The cache's answer for the schema being recalled: its lines read as
`describe`'s, if they make the pages the unit said; else describe it.
-}
recalled : { key : String, lines : Maybe (List String) } -> Model -> Model
recalled r m =
    case m.stage of
        Recalling s reply ->
            if r.key /= cacheKey s reply.hash then
                m

            else
                let
                    cached : Maybe Schema
                    cached =
                        r.lines
                            |> Maybe.map (List.foldl (\l acc -> read l acc) Schema.empty)
                            |> Maybe.andThen
                                (\sch ->
                                    if Schema.pageCount sch == Just reply.pages && List.length (Schema.pages sch) == reply.pages then
                                        Just sch

                                    else
                                        Nothing
                                )
                in
                case cached of
                    Just sch ->
                        readNext (install s { hash = reply.hash, schema = sch } m)

                    Nothing ->
                        { m | stage = Describing s reply Protocol.firstPage, incoming = Schema.empty, incomingLines = [] }

        Greeting ->
            m

        Checking _ _ ->
            m

        Describing _ _ _ ->
            m

        Following ->
            m

        Halted _ ->
            m


{-| One cached line into a schema, as `describe`'s answer is read.
-}
read : String -> Schema -> Schema
read raw sch =
    case parse raw of
        Answer a ->
            Schema.add a sch

        Ok_ _ ->
            sch

        OkMore _ ->
            sch

        Queued ->
            sch

        Err_ _ ->
            sch

        SetRefused _ _ ->
            sch

        SinceTooLarge ->
            sch

        Unknown ->
            sch


{-| A schema read: held for this connection, and shown where it belongs.
-}
install : SchemaId -> Held -> Model -> Model
install s h m =
    let
        m1 : Model
        m1 =
            { m | held = Dict.insert (schemaToken s) h m.held }
    in
    case s of
        MixerSchema ->
            { m1 | mixer = h.schema }

        EngineSchema e ->
            if m.engine == Just e then
                { m1 | schema = h.schema, described = Just e }

            else
                m1

        MatrixSchema e ->
            if m.engine == Just e then
                { m1 | matrix = h.schema }

            else
                m1


{-| What to read next: the followed engine's schema, the mixer's, then the
engine's matrix, each once a connection; then following.
-}
readNext : Model -> Model
readNext m =
    let
        lacks : SchemaId -> Bool
        lacks s =
            not (Dict.member (schemaToken s) m.held)

        go : Stage -> Model
        go st =
            { m | stage = st }
    in
    case m.engine of
        Just e ->
            if lacks (EngineSchema e) then
                go (Checking (EngineSchema e) Nothing)

            else if lacks MixerSchema then
                go (Checking MixerSchema Nothing)

            else if lacks (MatrixSchema e) then
                go (Checking (MatrixSchema e) Nothing)

            else
                go Following

        Nothing ->
            go (Halted UpdateEditor)
