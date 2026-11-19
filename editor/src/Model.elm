module Model exposing
    ( Model
    , Msg(..)
    , Reading
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

`step` is the pure core (messages in, commands out); `update` only sends them.

-}

import Addr exposing (Addr)
import Dict exposing (Dict)
import Protocol exposing (AnswerLine(..), Command(..), Line(..), PageNo, Value, encode, parse)
import Schema exposing (Schema)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine, Gen, Part)


{-| One value as the unit reported it: its text is the box's own.
-}
type alias Reading =
    { norm : Float, stored : Float, text : String }


{-| Where the connect sequence is. One request is in flight at a time.
`Describing` reads one engine's pages, one at a time.
-}
type Stage
    = Greeting
    | Describing Engine PageNo
    | Following
    | Halted Why


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
    , incoming : Schema
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


{-| The console protocol this editor speaks.
-}
supported : Int
supported =
    3


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
    , incoming = Schema.empty
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
        , Time.every pollMs Tick
        ]


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    let
        ( m, commands ) =
            step msg model
    in
    ( m, Cmd.batch (opening msg :: List.map send commands) )


{-| The one thing besides commands a message asks of the world: a link.
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


send : Command -> Cmd msg
send c =
    encode c |> Maybe.map Transport.send |> Maybe.withDefault Cmd.none


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

        Describing _ _ ->
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


step : Msg -> Model -> ( Model, List Command )
step msg m =
    case msg of
        LinkChanged l ->
            -- Any link change starts over: nothing of the last link is shown.
            ( { init | link = Just l, via = m.via, now = m.now, seat = next m.seat }, [] )

        Tick t ->
            tick t m

        Line raw ->
            ( line (parse raw) m, [] )

        Edit a v ->
            edit a v m

        -- By its number: a name with a space (GATE EXT) is one word so.
        Pick a i ->
            case Schema.optionName m.variants a i m.schema of
                Just _ ->
                    edit a (Protocol.Index i) m

                Nothing ->
                    ( m, [] )

        -- The box moves; the mark follows when `since` says so.
        FollowPart p ->
            if m.stage == Following then
                ( m, [ Follow p ] )

            else
                ( m, [] )

        -- Opening the link is `update`'s (it asks the shim).
        Connect v ->
            ( { m | via = Just v }, [] )


next : Seat -> Seat
next (Seat n) =
    Seat (n + 1)


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

            Describing e n ->
                connecting timedOut silent (ask (Describe e n)) m


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
    if takesEdits m && encode c /= Nothing then
        ( m, [ c ] )

    else
        ( m, [] )


line : Line -> Model -> Model
line l m =
    case m.stage of
        Halted _ ->
            m

        Greeting ->
            lineIn l m

        Describing _ _ ->
            lineIn l m

        Following ->
            lineIn l m


lineIn : Line -> Model -> Model
lineIn l m =
    case l of
        Answer a ->
            answer a m

        Ok_ done ->
            ok done m

        Err_ why ->
            if m.stage == Following then
                -- A refused `since` stays owed; the timeout re-asks, then settles it as lost.
                m

            else if m.stage == Greeting && String.startsWith "unknown command" why then
                { m | stage = Halted UpdateUnit, asked = Nothing }

            else
                { m | stage = Halted (RefusedBy why), asked = Nothing }

        SetRefused a why ->
            { m | refusals = Dict.insert (Addr.key a) why m.refusals }

        Queued ->
            m

        -- Asking again gets the same: stop, and say so.
        SinceTooLarge ->
            { m | stage = Halted TooLarge, asked = Nothing }

        Unknown ->
            m


answer : AnswerLine -> Model -> Model
answer a m =
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

        PartIs n name ->
            case Unit.engineFromName name of
                Just e ->
                    { m
                        | part = Unit.partFromInt n
                        , engine = Just e
                        , seat = next m.seat
                        , values = Dict.empty
                        , refusals = Dict.empty
                        , whole = True
                        , variants = Dict.empty
                        , source = Nothing
                    }

                Nothing ->
                    { m | stage = Halted UpdateEditor }

        Panel p ->
            if m.stage == Following then
                { m | variants = Dict.insert p.name p.variant m.variants }

            else
                { m | incoming = Schema.add a m.incoming }

        Val v ->
            if Schema.param v.addr m.schema == Nothing then
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
                { m | incoming = Schema.add a m.incoming }

        Page _ ->
            { m | incoming = Schema.add a m.incoming }

        Param _ ->
            { m | incoming = Schema.add a m.incoming }

        Opt _ ->
            { m | incoming = Schema.add a m.incoming }

        Route _ ->
            { m | incoming = Schema.add a m.incoming }


{-| `OK` ends the request in flight.
-}
ok : Maybe Int -> Model -> Model
ok done m0 =
    let
        m : Model
        m =
            { m0 | whole = False }
    in
    case m.stage of
        Greeting ->
            case m.engine of
                Just e ->
                    { m | stage = Describing e Protocol.firstPage, incoming = Schema.empty, asked = Nothing, owed = 0 }

                Nothing ->
                    { m | stage = Halted UpdateEditor, asked = Nothing }

        Describing e n ->
            if Schema.pageCount m.incoming |> Maybe.map (\c -> Protocol.pageNoToInt n + 1 < c) |> Maybe.withDefault False then
                { m | stage = Describing e (Protocol.nextPage n), asked = Nothing, owed = 0 }

            else
                -- The whole schema is in: it replaces the one shown.
                { m | stage = Following, schema = m.incoming, incoming = Schema.empty, described = Just e, asked = Nothing, owed = 0 }

        Following ->
            case done |> Maybe.andThen Unit.genFromInt of
                Just g ->
                    following g (max 0 (m.owed - 1)) m

                Nothing ->
                    m

        Halted _ ->
            m


{-| A `since` answered. Once nothing is owed, a box on another engine is
described afresh, then read from nothing; the old page stays up meanwhile.
-}
following : Gen -> Int -> Model -> Model
following g owed m =
    case ( owed, m.engine ) of
        ( 0, Just e ) ->
            if m.engine /= m.described then
                { m
                    | stage = Describing e Protocol.firstPage
                    , incoming = Schema.empty
                    , values = Dict.empty
                    , refusals = Dict.empty
                    , variants = Dict.empty
                    , gen = Nothing
                    , owed = 0
                    , leftover = False
                    , asked = Nothing
                }

            else
                answered g owed m

        _ ->
            answered g owed m


answered : Gen -> Int -> Model -> Model
answered g owed m =
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
