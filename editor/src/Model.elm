module Model exposing
    ( Effect(..)
    , Held
    , Model
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

`step` is the pure core (messages in, effects out); `update` only runs them.

The schemas are the unit's own text (ADR 0289): on connect, and on an engine
the editor hasn't read, it asks `ed schema <S>` for its page count and hash,
and uses its cached copy of that hash (IndexedDB, `Cache`) or describes every
page and keeps them. It reads the followed engine's schema and the MIXER's.

-}

import Cache
import Dict exposing (Dict)
import Protocol exposing (Addr, AnswerLine(..), Command(..), Hash, Line(..), OptIndex, SchemaId(..), Value, addrKey, encode, hashText, parse, schemaToken)
import Schema exposing (Schema)
import Time
import Transport exposing (Link(..))
import Unit exposing (Engine(..), Gen, Part)


{-| One value as the unit reported it: its text is the box's own.
-}
type alias Reading =
    { norm : Float, stored : Float, text : String }


{-| Where the connect sequence is. One request is in flight at a time.
A schema is checked (`ed schema`), recalled from the cache, or described page
by page; then the next one the editor lacks, then following.
-}
type Stage
    = Greeting
    | Checking SchemaId (Maybe Reply)
    | Recalling SchemaId Reply
    | Describing SchemaId Reply Int
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
    | Refused String
    | Silent
    | TooLarge


type alias Model =
    { link : Maybe Link
    , via : Maybe Via
    , schema : Schema
    , mixer : Schema
    , held : Dict String Held
    , building : Schema
    , described : List String
    , values : Dict String Reading
    , pending : Dict String Time.Posix
    , part : Maybe Part
    , engine : Maybe Engine
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
    | Recalled { key : String, lines : Maybe (List String) }


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
    , mixer = Schema.empty
    , held = Dict.empty
    , building = Schema.empty
    , described = []
    , values = Dict.empty
    , pending = Dict.empty
    , part = Nothing
    , engine = Nothing
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
    case msg of
        ConnectSerial ->
            ( { model | via = Just ViaUnit }, Transport.connectSerial () )

        ConnectSocket ->
            ( { model | via = Just ViaSimulator }, Transport.connectSocket "ws://127.0.0.1:7342" )

        LinkChanged _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        Line _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        Tick _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        Edit _ _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        Pick _ _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        FollowPart _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)

        Recalled _ ->
            step msg model |> Tuple.mapSecond (List.map run >> Cmd.batch)


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


step : Msg -> Model -> ( Model, List Effect )
step msg m =
    case msg of
        LinkChanged l ->
            -- Any link change starts over: nothing of the last link is shown.
            ( { init | link = Just l, via = m.via, now = m.now }, [] )

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

        -- Connecting is `update`'s (it asks the shim).
        ConnectSerial ->
            ( m, [] )

        ConnectSocket ->
            ( m, [] )

        Recalled r ->
            ( recalled r m, [] )


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
    Schema.param a m.schema /= Nothing || Schema.param a m.mixer /= Nothing


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
    if m.stage == Following && isDescribed a m && encode c /= Nothing then
        ( { m | pending = Dict.insert (addrKey a) m.now m.pending }, [ c ] )

    else
        ( m, [] )


line : String -> Line -> Model -> ( Model, List Effect )
line raw l m =
    if isHalted m then
        ( m, [] )

    else
        lineIn raw l m


isHalted : Model -> Bool
isHalted m =
    case m.stage of
        Halted _ ->
            True

        Greeting ->
            False

        Checking _ _ ->
            False

        Recalling _ _ ->
            False

        Describing _ _ _ ->
            False

        Following ->
            False


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
                    -- Ignored: a `set`'s ERR looks the same. A refused `since` stays owed; the timeout re-asks, then settles it as lost.
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

        Queued ->
            ( m, [] )

        -- Asking again gets the same: stop, and say so.
        SinceTooLarge ->
            ( { m | stage = Halted TooLarge, asked = Nothing }, [] )

        Unknown ->
            ( m, [] )


refused : String -> Model -> Model
refused why m =
    { m | stage = Halted (Refused why), asked = Nothing }


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
                    -- A new Part or engine: PART's section is whole.
                    { m
                        | part = Unit.partFromInt n
                        , engine = Just e
                        , schema = Dict.get (Unit.engineName e) m.held |> Maybe.map .schema |> Maybe.withDefault Schema.empty
                        , values = Dict.filter (\k _ -> Schema.param (Protocol.Addr k) m.mixer /= Nothing) m.values
                        , variants = Dict.filter (\k _ -> Schema.hasPanel k m.mixer) m.variants
                        , pending = Dict.filter (\k _ -> Schema.param (Protocol.Addr k) m.mixer /= Nothing) m.pending
                        , source = Nothing
                    }

                Nothing ->
                    { m | stage = Halted UpdateEditor }

        MixerIs ->
            -- MIXER's section whole: what was held for it goes.
            { m
                | values = Dict.filter (\k _ -> Schema.param (Protocol.Addr k) m.mixer == Nothing) m.values
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
                    | values = Dict.insert (addrKey v.addr) { norm = v.norm, stored = v.stored, text = v.text } m.values
                    , pending = Dict.remove (addrKey v.addr) m.pending
                }

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


{-| A line of `ed describe`'s answer: into the schema being read, and kept
as the unit wrote it for the cache.
-}
describing : String -> AnswerLine -> Model -> Model
describing raw a m =
    case m.stage of
        Describing _ _ _ ->
            { m | building = Schema.add a m.building, described = raw :: m.described }

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
ok done more m =
    case m.stage of
        Greeting ->
            ( next { m | asked = Nothing, owed = 0 }, [] )

        Checking s (Just r) ->
            let
                m1 : Model
                m1 =
                    { m | asked = Nothing, owed = 0 }
            in
            case Dict.get (schemaToken s) m.held of
                Just h ->
                    if h.hash == r.hash then
                        ( next (install s h m1), [] )

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
            if n + 1 < r.pages then
                ( { m1 | stage = Describing s r (n + 1) }, [] )

            else
                let
                    h : Held
                    h =
                        { hash = r.hash, schema = m.building }
                in
                ( next (install s h { m1 | building = Schema.empty, described = [] })
                , [ Keep (cacheKey s r.hash) (List.reverse m.described) ]
                )

        Following ->
            case done |> Maybe.andThen Unit.genFromInt of
                Just g ->
                    let
                        owed : Int
                        owed =
                            max 0 (m.owed - 1)

                        m1 : Model
                        m1 =
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
                    in
                    -- An engine the editor hasn't read: read it once nothing is owed.
                    if owed == 0 && not more then
                        ( next m1, [] )

                    else
                        ( m1, [] )

                Nothing ->
                    ( m, [] )

        Halted _ ->
            ( m, [] )


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
                        next (install s { hash = reply.hash, schema = sch } m)

                    Nothing ->
                        { m | stage = Describing s reply 0, building = Schema.empty, described = [] }

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
                { m1 | schema = h.schema }

            else
                m1


{-| What to read next: the followed engine's schema, then the mixer's, each
once a connection; then following.
-}
next : Model -> Model
next m =
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

            else
                go Following

        Nothing ->
            go (Halted UpdateEditor)
