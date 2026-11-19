module Model exposing
    ( Model
    , Msg(..)
    , Reading
    , Stage
    , Via
    , init
    , isPending
    , notice
    , shown
    , status
    , step
    , subscriptions
    , update
    )

{-| Only the unit's state: what it last reported, and which edits wait to be
confirmed. The editor never guesses a value; a control always shows the unit's.

`step` is the pure core (messages in, commands out); `update` only sends them.

-}

import Dict exposing (Dict)
import Protocol exposing (Addr, AnswerLine(..), Command(..), Line(..), OptIndex, Value, addrKey, encode, parse)
import Schema exposing (Schema)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine, Gen, Part)


{-| One value as the unit reported it: its text is the box's own.
-}
type alias Reading =
    { norm : Float, stored : Float, text : String }


{-| Where the connect sequence is. One request is in flight at a time.
-}
type Stage
    = Greeting
    | Describing Int
    | Following
    | Halted Why


type Why
    = UpdateEditor
    | UpdateUnit
    | Refused String
    | Silent
    | TooLarge


type alias Model =
    { link : Maybe Link
    , via : Maybe Via
    , schema : Schema
    , values : Dict String Reading
    , pending : Dict String Time.Posix
    , part : Maybe Part
    , engine : Maybe Engine
    , described : Maybe Engine
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
    | Pick Addr OptIndex
    | FollowPart Part
    | ConnectSerial
    | ConnectSocket


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


pendingMs : Int
pendingMs =
    250


pollMs : Float
pollMs =
    33


init : Model
init =
    { link = Nothing
    , via = Nothing
    , schema = Schema.empty
    , values = Dict.empty
    , pending = Dict.empty
    , part = Nothing
    , engine = Nothing
    , described = Nothing
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
    case msg of
        ConnectSerial ->
            ( { model | via = Just ViaUnit }, Transport.connectSerial () )

        ConnectSocket ->
            ( { model | via = Just ViaSimulator }, Transport.connectSocket "ws://127.0.0.1:7342" )

        LinkChanged _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)

        Line _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)

        Tick _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)

        Edit _ _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)

        Pick _ _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)

        FollowPart _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)


send : Command -> Cmd msg
send c =
    encode c |> Maybe.map Transport.send |> Maybe.withDefault Cmd.none


{-| Once connected and following: the link and the Part and engine it follows.
-}
status : Model -> Maybe String
status m =
    case ( ( m.link, m.stage ), ( m.part, m.engine ) ) of
        ( ( Just Open, Following ), ( Just p, Just e ) ) ->
            Just
                (String.join " · "
                    [ if m.via == Just ViaSimulator then
                        "SIMULATOR"

                      else
                        "UNIT"
                    , "PART " ++ String.fromInt (Unit.partNumber p)
                    , Unit.engineName e
                    ]
                )

        _ ->
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

        Halted (Refused t) ->
            Just ("THE UNIT REFUSED: " ++ t)

        Halted Silent ->
            Just "THE UNIT STOPPED ANSWERING"

        Halted TooLarge ->
            Just "THE UNIT'S ANSWER IS TOO LARGE FOR ONE READ"

        Greeting ->
            Nothing

        Describing _ ->
            Nothing

        Following ->
            Nothing


shown : Addr -> Model -> Maybe Reading
shown a m =
    Dict.get (addrKey a) m.values


isPending : Addr -> Model -> Bool
isPending a m =
    Dict.member (addrKey a) m.pending


step : Msg -> Model -> ( Model, List Command )
step msg m =
    case msg of
        LinkChanged l ->
            -- Any link change starts over: nothing of the last link is shown.
            ( { init | link = Just l, via = m.via, now = m.now }, [] )

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

        -- Connecting is `update`'s (it asks the shim).
        ConnectSerial ->
            ( m, [] )

        ConnectSocket ->
            ( m, [] )


tick : Time.Posix -> Model -> ( Model, List Command )
tick t m0 =
    let
        m : Model
        m =
            { m0
                | now = t
                , pending = Dict.filter (\_ at -> ms t - ms at < pendingMs) m0.pending
            }

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

            Describing n ->
                case m.described of
                    Just e ->
                        connecting timedOut silent (ask (Describe e n)) m

                    Nothing ->
                        ( m, [] )


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
    if m.stage == Following && encode c /= Nothing then
        ( { m | pending = Dict.insert (addrKey a) m.now m.pending }, [ c ] )

    else
        ( m, [] )


line : Line -> Model -> Model
line l m =
    if isHalted m then
        m

    else
        lineIn l m


isHalted : Model -> Bool
isHalted m =
    case m.stage of
        Halted _ ->
            True

        Greeting ->
            False

        Describing _ ->
            False

        Following ->
            False


lineIn : Line -> Model -> Model
lineIn l m =
    case l of
        Answer a ->
            answer a m

        Ok_ done ->
            ok done m

        Err_ why ->
            case m.stage of
                Following ->
                    -- Ignored: a `set`'s ERR looks the same. A refused `since` stays owed; the timeout re-asks, then settles it as lost.
                    m

                Greeting ->
                    if m.stage == Greeting && String.startsWith "unknown command" why then
                        { m | stage = Halted UpdateUnit, asked = Nothing }

                    else
                        { m | stage = Halted (Refused why), asked = Nothing }

                Describing _ ->
                    if m.stage == Greeting && String.startsWith "unknown command" why then
                        { m | stage = Halted UpdateUnit, asked = Nothing }

                    else
                        { m | stage = Halted (Refused why), asked = Nothing }

                Halted _ ->
                    if m.stage == Greeting && String.startsWith "unknown command" why then
                        { m | stage = Halted UpdateUnit, asked = Nothing }

                    else
                        { m | stage = Halted (Refused why), asked = Nothing }

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
                        , values = Dict.empty
                        , variants = Dict.empty
                        , pending = Dict.empty
                        , source = Nothing
                    }

                Nothing ->
                    { m | stage = Halted UpdateEditor }

        Panel p ->
            if m.stage == Following then
                { m | variants = Dict.insert p.name p.variant m.variants }

            else
                { m | schema = Schema.add a m.schema }

        Val v ->
            if Schema.param v.addr m.schema == Nothing then
                -- The unit says nothing of it in `describe`: not the editor's.
                m

            else
                { m
                    | values = Dict.insert (addrKey v.addr) { norm = v.norm, stored = v.stored, text = v.text } m.values
                    , pending = Dict.remove (addrKey v.addr) m.pending
                }

        Source text ->
            if m.stage == Following then
                { m | source = Just text }

            else
                { m | schema = Schema.add a m.schema }

        Page _ ->
            { m | schema = Schema.add a m.schema }

        Param _ ->
            { m | schema = Schema.add a m.schema }

        Opt _ ->
            { m | schema = Schema.add a m.schema }

        Route _ ->
            { m | schema = Schema.add a m.schema }


{-| `OK` ends the request in flight.
-}
ok : Maybe Int -> Model -> Model
ok done m =
    case m.stage of
        Greeting ->
            { m | stage = Describing 0, described = m.engine, asked = Nothing, owed = 0 }

        Describing n ->
            let
                more : Bool
                more =
                    Schema.pageCount m.schema |> Maybe.map (\c -> n + 1 < c) |> Maybe.withDefault False
            in
            { m | stage = nextPage n more, asked = Nothing, owed = 0 }

        Following ->
            case done |> Maybe.andThen Unit.genFromInt of
                Just g ->
                    let
                        owed : Int
                        owed =
                            max 0 (m.owed - 1)
                    in
                    if owed == 0 && m.engine /= m.described then
                        -- The box follows a Part on another engine: read its schema, then everything.
                        { m
                            | stage = Describing 0
                            , described = m.engine
                            , schema = Schema.empty
                            , values = Dict.empty
                            , variants = Dict.empty
                            , gen = Nothing
                            , owed = 0
                            , leftover = False
                            , asked = Nothing
                        }

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

                Nothing ->
                    m

        Halted _ ->
            m


nextPage : Int -> Bool -> Stage
nextPage n more =
    if more then
        Describing (n + 1)

    else
        Following
