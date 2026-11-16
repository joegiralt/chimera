module Model exposing
    ( Model
    , Msg(..)
    , Reading
    , init
    , isPending
    , notice
    , shown
    , step
    , subscriptions
    , update
    )

{-| Only the unit's state: what it last reported, and which edits wait to be
confirmed. The editor never guesses a value; a control always shows the unit's.

`step` is the pure core (messages in, commands out); `update` only sends them.

-}

import Dict exposing (Dict)
import Protocol exposing (Addr, AnswerLine(..), Command(..), Line(..), Value, addrKey, encode, parse)
import Schema exposing (Schema)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine(..), Gen, Part)


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


type alias Model =
    { link : Maybe Link
    , schema : Schema
    , values : Dict String Reading
    , pending : Dict String Time.Posix
    , part : Maybe Part
    , engine : Maybe Engine
    , variants : Dict String String
    , gen : Maybe Gen
    , stage : Stage
    , asked : Maybe Time.Posix
    , owed : Int
    , now : Time.Posix
    }


type Msg
    = LinkChanged Link
    | Line String
    | Tick Time.Posix
    | Edit Addr Value
    | Pick Addr Int
    | ConnectSerial
    | ConnectSocket


{-| The console protocol this editor speaks.
-}
supported : Int
supported =
    2


{-| A request unanswered this long has timed out. While connecting, that
halts: a late `OK` could not be told from the next request's, so it would
skip a step. Following, a `since` is asked once more with the last number we
did get, and no further until every answer owed has come (`owed`); if that
re-ask is lost too, the count resets and it asks afresh, never halting (the
unit heals a lost answer on the next ask). The unit answers in order, so a late `OK` is only
counted, and its `val` lines are true, as the number only moves forward.
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
    , schema = Schema.empty
    , values = Dict.empty
    , pending = Dict.empty
    , part = Nothing
    , engine = Nothing
    , variants = Dict.empty
    , gen = Nothing
    , stage = Greeting
    , asked = Nothing
    , owed = 0
    , now = Time.millisToPosix 0
    }


subscriptions : Model -> Sub Msg
subscriptions _ =
    Sub.batch
        [ Transport.lineIn Line
        , Transport.linkState (Transport.parseLink >> LinkChanged)
        , Time.every pollMs Tick
        ]


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        ConnectSerial ->
            ( model, Transport.connectSerial () )

        ConnectSocket ->
            ( model, Transport.connectSocket "ws://127.0.0.1:7342" )

        _ ->
            step msg model |> Tuple.mapSecond (List.map send >> Cmd.batch)


send : Command -> Cmd msg
send c =
    encode c |> Maybe.map Transport.send |> Maybe.withDefault Cmd.none


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

        _ ->
            case ( m.part, m.engine ) of
                ( Just p, Just e ) ->
                    if e /= Algo then
                        Just ("PART " ++ String.fromInt (Unit.partNumber p) ++ " PLAYS " ++ Unit.engineName e ++ ". THE EDITOR EDITS ALGO FOR NOW.")

                    else
                        Nothing

                _ ->
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
            ( { init | link = Just l, now = m.now }, [] )

        Tick t ->
            tick t m

        Line raw ->
            ( line (parse raw) m, [] )

        Edit a v ->
            edit a v m

        Pick a i ->
            case Schema.optionName a i m.schema of
                Just name ->
                    edit a (Protocol.Name name) m

                Nothing ->
                    ( m, [] )

        -- Connecting is `update`'s: it only asks the shim.
        _ ->
            ( m, [] )


tick : Time.Posix -> Model -> ( Model, List Command )
tick t m0 =
    let
        m =
            { m0
                | now = t
                , pending = Dict.filter (\_ at -> ms t - ms at < pendingMs) m0.pending
            }

        timedOut =
            m.asked |> Maybe.map (\at -> ms t - ms at >= answerTimeoutMs) |> Maybe.withDefault False

        ask c =
            ( { m | asked = Just t, owed = m.owed + 1 }, [ c ] )

        silent =
            ( { m | stage = Halted Silent, asked = Nothing }, [] )
    in
    if m.link /= Just Open then
        ( m, [] )

    else
        case m.stage of
            Halted _ ->
                ( m, [] )

            Following ->
                if m.owed == 0 then
                    ask (Since m.gen)

                else if timedOut && m.owed == 1 then
                    ask (Since m.gen)

                else if timedOut then
                    -- The re-ask went unanswered too: count the answers lost and ask afresh. The unit heals a lost answer on the next ask.
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
                connecting timedOut silent (ask (Describe "ALGO" n)) m


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
        c =
            Set a v
    in
    if m.stage == Following && m.engine == Just Algo && encode c /= Nothing then
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

        _ ->
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
                    -- Ignored: a `set`'s ERR looks the same, and only the `OK` counts a `since` answered (a lost one times out).
                    m

                _ ->
                    { m | stage = Halted (Refused why), asked = Nothing }

        Queued ->
            m

        Unknown _ ->
            m


answer : AnswerLine -> Model -> Model
answer a m =
    case a of
        HelloIs h ->
            if h.protocol == supported then
                { m | part = Unit.partFromInt h.part, engine = Just (Unit.engineFromName h.engine) }

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

        PartIs n e ->
            { m
                | part = Unit.partFromInt n
                , engine = Just (Unit.engineFromName e)
                , values = Dict.empty
                , variants = Dict.empty
                , pending = Dict.empty
            }

        Panel p ->
            if m.stage == Following then
                { m | variants = Dict.insert p.name p.variant m.variants }

            else
                { m | schema = Schema.add a m.schema }

        Val v ->
            { m
                | values = Dict.insert (addrKey v.addr) { norm = v.norm, stored = v.stored, text = v.text } m.values
                , pending = Dict.remove (addrKey v.addr) m.pending
            }

        _ ->
            { m | schema = Schema.add a m.schema }


{-| `OK` ends the request in flight.
-}
ok : Maybe Int -> Model -> Model
ok done m =
    case m.stage of
        Greeting ->
            { m | stage = Describing 0, asked = Nothing, owed = 0 }

        Describing n ->
            let
                more =
                    Schema.pageCount m.schema |> Maybe.map (\c -> n + 1 < c) |> Maybe.withDefault False
            in
            { m | stage = nextPage n more, asked = Nothing, owed = 0 }

        Following ->
            case done |> Maybe.andThen Unit.genFromInt of
                Just g ->
                    { m | gen = Just g, owed = max 0 (m.owed - 1), asked = Nothing }

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
