module Model exposing
    ( Effect(..)
    , Held
    , Kept
    , LearnReading
    , Listed
    , Listing
    , MapHeader
    , MapRowReading
    , Model
    , Msg(..)
    , Notice(..)
    , Reading
    , RouteReading
    , Seat
    , Stage
    , Via(..)
    , armedOn
    , init
    , lastRefusal
    , notice
    , noticeText
    , notices
    , noticesLost
    , refusal
    , seat
    , sensingRow
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

Nothing the unit says is dropped unseen. It answers each command line in the
order sent, so the commands still unanswered are held in that order
(`awaiting`) and an `ERR` is tied to the one it answers. Every `ERR`, and
every line the editor can't read, is kept as a `Notice` until the user has
seen it (`SeenNotices`): no later answer, success or link erases one.

The schemas are the unit's own text (ADR 0289): on connect, and on an engine
the editor hasn't read, it asks `ed schema <S>` for its page count and hash,
and uses its cached copy of that hash (IndexedDB, `Cache`) or describes every
page and keeps them. A schema is the text its hash names: a cached copy whose
lines don't hash to the unit's is described again, and a described one that
doesn't, or that holds a line the editor can't read, halts and says so. It reads the followed engine's schema, the MIXER's, the
engine's matrix, SETTINGS', MIDI MAP's and ORBIT's.

-}

import Addr exposing (Addr)
import Cache
import Dict exposing (Dict)
import Hash exposing (Hash)
import Protocol exposing (AnswerLine(..), Cell, Command(..), Degree, Edits, Generation, Health, Learn, Line(..), MapCommand(..), MapInput, MapKind, MapRow(..), Note, NotePool, OnOff, OrbitCommand, OverEdits, PageNo, ProjectFile, ProjectId, ProjectName, RouteTo, RowStatus, ScaleMask, SchemaId(..), Slot, Song, SoundStatus, SpanPlace, Value, cellKey, encode, parse, ringNoToInt, rowIndexToInt, sceneNoToInt, schemaToken, segmentToInt, stepNoToInt)
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


{-| A project file as `since`'s listing says it.
-}
type alias Listed =
    { id : ProjectId, name : String, health : Health, generation : Maybe Generation }


{-| The card's projects as last listed (`ed projects`), in id order, and
whether the card has more than the box lists.
-}
type alias Listing =
    { projects : List Listed, more : Bool }


{-| MIDI MAP's header: the followed Part, its controller map (none picked:
`Nothing`) and KEEP MAP.
-}
type alias MapHeader =
    { part : Part, name : Maybe String, keep : OnOff }


{-| A MIDI MAP row as `since` said it.
-}
type alias MapRowReading =
    { input : MapInput, kind : MapKind, addr : Maybe Addr, min : SpanPlace, max : SpanPlace, echo : OnOff, status : RowStatus }


{-| The box's MIDI learn and its focus-band words.
-}
type alias LearnReading =
    { learn : Learn, note : String }


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
    | BadSchema SchemaId SchemaFault


{-| Why a schema's text can't be used: a line of it the editor can't read,
or text that is not what the unit's hash names (what it said, what was read).
-}
type SchemaFault
    = Unreadable String
    | WrongHash Hash Hash


{-| Something the unit said that the editor could not act on: a refusal, with
the command it answers where one was waiting, or a line it can't read, as
the unit wrote it.
-}
type Notice
    = Refusal { sent : Maybe Command, why : String }
    | Unread String


{-| A notice kept for the user, and how many times the unit said it.
-}
type alias Kept =
    { notice : Notice, times : Int }


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
    , settings : Schema
    , midiMap : Schema
    , orbit : Schema
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

    -- The commands sent while following that the unit has not answered,
    -- oldest first: it answers in the order sent.
    , awaiting : List Command

    -- What the unit refused or the editor couldn't read, newest first, each
    -- distinct one once; and how many fell off the end unseen.
    , kept : List Kept
    , lost : Int

    -- SETTINGS' section: the loaded project, each Part's Sound (by its
    -- number), the box's last toast, the listing.
    , project : Maybe { name : String, file : ProjectFile, edits : Edits, generation : Maybe Generation }
    , sounds : Dict Int { name : String, status : SoundStatus }
    , toast : Maybe String
    , listing : Maybe Listing

    -- MAP's section: its header, the Sound's rows and the controller map's
    -- (by their 1-based number), learn, the card's maps as last listed.
    , mapHeader : Maybe MapHeader
    , soundRows : Dict Int MapRowReading
    , mapRows : Dict Int MapRowReading
    , learn : Maybe LearnReading
    , mapFiles : List String

    -- ORBIT's section: the song, each scene slot, each segment's chord (by
    -- their 1-based numbers), CUSTOM's scale, each ring's steps (by ring and
    -- step) and pools (by ring).
    , song : Maybe Song
    , slots : Dict Int Slot
    , chords : Dict Int Degree
    , scale : Maybe ScaleMask
    , steps : Dict ( Int, Int ) Note
    , pools : Dict Int NotePool
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
      -- The card's projects listed again.
    | AskProjects
      -- Save over the loaded project's own file.
    | SaveOwn
      -- Save into a new id.
    | SaveNew ProjectName
      -- Load a listed file, witnessed by the generation the listing said.
    | LoadListed ProjectId Generation OverEdits
      -- A MIDI MAP or learn command.
    | MapAct MapCommand
      -- An ORBIT transport, scene, chord, scale, step or pool command.
    | OrbitAct OrbitCommand
      -- The user has seen the notices: they go.
    | SeenNotices


{-| The console protocol this editor speaks.
-}
supported : Int
supported =
    4


{-| A schema's cache key: the protocol, the schema and its hash.
-}
cacheKey : SchemaId -> Hash -> String
cacheKey s h =
    String.join "/" [ String.fromInt supported, schemaToken s, Hash.toHex h ]


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
    , settings = Schema.empty
    , midiMap = Schema.empty
    , orbit = Schema.empty
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
    , awaiting = []
    , kept = []
    , lost = 0
    , project = Nothing
    , sounds = Dict.empty
    , toast = Nothing
    , listing = Nothing
    , mapHeader = Nothing
    , soundRows = Dict.empty
    , mapRows = Dict.empty
    , learn = Nothing
    , mapFiles = []
    , song = Nothing
    , slots = Dict.empty
    , chords = Dict.empty
    , scale = Nothing
    , steps = Dict.empty
    , pools = Dict.empty
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

        AskProjects ->
            Cmd.none

        SaveOwn ->
            Cmd.none

        SaveNew _ ->
            Cmd.none

        LoadListed _ _ _ ->
            Cmd.none

        MapAct _ ->
            Cmd.none

        OrbitAct _ ->
            Cmd.none

        SeenNotices ->
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

        Halted (BadSchema s (Unreadable l)) ->
            Just ("THE UNIT'S " ++ schemaToken s ++ " SCHEMA HAS A LINE THIS EDITOR CAN'T READ: " ++ l)

        Halted (BadSchema s (WrongHash said read_)) ->
            Just ("THE UNIT'S " ++ schemaToken s ++ " SCHEMA IS NOT THE TEXT ITS HASH NAMES: " ++ Hash.toHex said ++ ", READ " ++ Hash.toHex read_)

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


{-| What the unit refused or the editor couldn't read and the user has not
yet seen, newest first.
-}
notices : Model -> List Kept
notices m =
    m.kept


{-| How many notices fell off the end of `notices` unseen.
-}
noticesLost : Model -> Int
noticesLost m =
    m.lost


{-| The newest refusal the user has not seen: the command it answers, where
one was waiting, and the unit's words.
-}
lastRefusal : Model -> Maybe { sent : Maybe Command, why : String }
lastRefusal m =
    m.kept
        |> List.filterMap
            (\k ->
                case k.notice of
                    Refusal r ->
                        Just r

                    Unread _ ->
                        Nothing
            )
        |> List.head


{-| A notice as the UI says it: the line sent, where known, and the unit's
own words.
-}
noticeText : Notice -> String
noticeText n =
    case n of
        Refusal r ->
            case r.sent |> Maybe.andThen encode of
                Just sent ->
                    "THE UNIT REFUSED " ++ sent ++ ": " ++ r.why

                Nothing ->
                    "THE UNIT REFUSED: " ++ r.why

        Unread raw ->
            "THE UNIT SAID A LINE THIS EDITOR CAN'T READ: " ++ raw


{-| At most this many distinct notices are kept; past it the oldest falls
off and is counted (`noticesLost`).
-}
noticesKept : Int
noticesKept =
    16


{-| A notice kept: one the user has not seen yet is counted once more and
comes first; else it is new, and past `noticesKept` the oldest is lost.
-}
keep : Notice -> Model -> Model
keep n m =
    let
        times : Int
        times =
            m.kept |> List.filter (\k -> k.notice == n) |> List.map .times |> List.sum

        all : List Kept
        all =
            { notice = n, times = times + 1 } :: List.filter (\k -> k.notice /= n) m.kept
    in
    { m | kept = List.take noticesKept all, lost = m.lost + max 0 (List.length all - noticesKept) }


{-| The cell the box's MIDI learn is armed on, when the box can name it.
-}
armedOn : Model -> Maybe Addr
armedOn m =
    case m.learn |> Maybe.map .learn of
        Just (Protocol.Armed a) ->
            a

        Just Protocol.NotLearning ->
            Nothing

        Just (Protocol.Sensing _) ->
            Nothing

        Just Protocol.Pointing ->
            Nothing

        Nothing ->
            Nothing


{-| The Sound row (1-based) learn is hearing a control onto.
-}
sensingRow : Model -> Maybe Int
sensingRow m =
    case m.learn |> Maybe.map .learn of
        Just (Protocol.Sensing i) ->
            Just (rowIndexToInt i)

        Just Protocol.NotLearning ->
            Nothing

        Just (Protocol.Armed _) ->
            Nothing

        Just Protocol.Pointing ->
            Nothing

        Nothing ->
            Nothing


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
    let
        ( m1, effects ) =
            stepIn msg m
    in
    ( if m.stage == Following then
        { m1 | awaiting = m1.awaiting ++ List.filterMap sentBy effects }

      else
        m1
    , effects
    )


{-| The command an effect puts on the wire, if it does (`run` sends only
what `encode` makes a line of).
-}
sentBy : Effect -> Maybe Command
sentBy e =
    case e of
        Send c ->
            encode c |> Maybe.map (\_ -> c)

        Recall _ ->
            Nothing

        Keep _ _ ->
            Nothing


stepIn : Msg -> Model -> ( Model, List Effect )
stepIn msg m =
    case msg of
        LinkChanged l ->
            -- Any link change starts over: nothing of the last link is shown
            -- but the notices the user has not seen.
            ( { init | link = Just l, via = m.via, now = m.now, seat = nextSeat m.seat, kept = m.kept, lost = m.lost }, [] )

        SeenNotices ->
            ( { m | kept = [], lost = 0 }, [] )

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

        -- The project commands: the box's own card work, under BUSY; what
        -- happened comes back through `since` (`project`, `projects`, `toast`).
        AskProjects ->
            projectCommand ListProjects m

        SaveOwn ->
            projectCommand SaveProject m

        SaveNew n ->
            projectCommand (SaveProjectAs n) m

        LoadListed id g over ->
            projectCommand (LoadProject id g over) m

        MapAct c ->
            mapCommand c m

        -- ORBIT's commands read no engine's schema; `since` says what came of them.
        OrbitAct c ->
            projectCommand (OrbitCmd c) m


{-| A MIDI MAP command goes out while following; one naming a cell only
against the engine the box is on, as an edit does.
-}
mapCommand : MapCommand -> Model -> ( Model, List Effect )
mapCommand c m =
    let
        names : Bool
        names =
            case c of
                AddRow _ _ _ ->
                    True

                LearnAt _ ->
                    True

                SetRow _ _ ->
                    False

                RemoveRow _ _ _ ->
                    False

                PickMap _ ->
                    False

                KeepMap _ ->
                    False

                SaveMap _ ->
                    False

                RenameMap _ ->
                    False

                ListMaps ->
                    False

                StopLearning ->
                    False
    in
    if names && not (takesEdits m) then
        ( m, [] )

    else
        projectCommand (MapCmd c) m


{-| A project command goes out while following: it reads no engine's schema.
-}
projectCommand : Command -> Model -> ( Model, List Effect )
projectCommand c m =
    if m.stage == Following then
        ( m, [ Send c ] )

    else
        ( m, [] )


nextSeat : Seat -> Seat
nextSeat (Seat n) =
    Seat (n + 1)


{-| The schema that describes `a`: the followed engine's, else the mixer's,
else ORBIT's, else SETTINGS'.
-}
schemaOf : Addr -> Model -> Schema
schemaOf a m =
    case ( Schema.param a m.schema, Schema.param a m.mixer, isOrbit a m ) of
        ( Just _, _, _ ) ->
            m.schema

        ( Nothing, Just _, _ ) ->
            m.mixer

        ( Nothing, Nothing, True ) ->
            m.orbit

        ( Nothing, Nothing, False ) ->
            m.settings


{-| Some schema the editor holds describes `a`.
-}
isDescribed : Addr -> Model -> Bool
isDescribed a m =
    Schema.param a m.schema /= Nothing || isMixer a m || isSettings a m || isOrbit a m


isOrbit : Addr -> Model -> Bool
isOrbit a m =
    Schema.param a m.orbit /= Nothing


isSettings : Addr -> Model -> Bool
isSettings a m =
    Schema.param a m.settings /= Nothing


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
                    ( { fresh | owed = 1, awaiting = [] }, cs )

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
        -- Nothing more is read, but nothing said is dropped.
        Halted _ ->
            case l of
                Err_ why ->
                    ( keep (Refusal { sent = Nothing, why = why }) m, [] )

                Unknown ->
                    ( unread raw m, [] )

                Answer _ ->
                    ( m, [] )

                Ok_ _ ->
                    ( m, [] )

                OkMore _ ->
                    ( m, [] )

                Queued ->
                    ( m, [] )

                SetRefused _ _ ->
                    ( m, [] )

                SinceTooLarge ->
                    ( m, [] )

        Greeting ->
            lineIn raw l m

        Checking _ _ ->
            lineIn raw l m

        Recalling _ _ ->
            lineIn raw l m

        Describing s _ _ ->
            case l of
                -- Describe's own text, whatever it is: the hash reads it all.
                Answer _ ->
                    lineIn raw l { m | incomingLines = raw :: m.incomingLines }

                Unknown ->
                    if String.trim raw == "" then
                        ( m, [] )

                    else
                        ( { m | stage = Halted (BadSchema s (Unreadable raw)), asked = Nothing }, [] )

                Ok_ _ ->
                    lineIn raw l m

                OkMore _ ->
                    lineIn raw l m

                Queued ->
                    lineIn raw l m

                Err_ _ ->
                    lineIn raw l m

                SetRefused _ _ ->
                    lineIn raw l m

                SinceTooLarge ->
                    lineIn raw l m

        Following ->
            lineIn raw l m


{-| A line the editor can't read, kept as the unit wrote it. A blank line
says nothing: it is no line.
-}
unread : String -> Model -> Model
unread raw m =
    if String.trim raw == "" then
        m

    else
        keep (Unread raw) m


{-| What kind of answer a command gets, as far as its last line tells: a
`since`'s `OK` carries its number, a set's refusal its address (the set of
that address it answers).
-}
type Asked
    = AskedSince
    | AskedSet Addr
    | AskedOther


askedBy : Command -> Asked
askedBy c =
    case c of
        Since _ ->
            AskedSince

        Set a _ ->
            AskedSet a

        Hello ->
            AskedOther

        AskSchema _ ->
            AskedOther

        Describe _ _ ->
            AskedOther

        Get _ ->
            AskedOther

        Follow _ ->
            AskedOther

        SetRoute _ _ ->
            AskedOther

        ListProjects ->
            AskedOther

        SaveProject ->
            AskedOther

        SaveProjectAs _ ->
            AskedOther

        LoadProject _ _ _ ->
            AskedOther

        MapCmd _ ->
            AskedOther

        OrbitCmd _ ->
            AskedOther


{-| An answer's last line came, and it can only be the answer to a command
`fits`: the oldest such one is answered, and any sent before it have lost
their answers (the unit answers in order). With none waiting, nothing moves.
-}
answered : (Asked -> Bool) -> Model -> Model
answered fits m =
    let
        after : List Command -> List Command
        after cs =
            case cs of
                c :: rest ->
                    if fits (askedBy c) then
                        rest

                    else
                        after rest

                [] ->
                    m.awaiting
    in
    { m | awaiting = after m.awaiting }


lineIn : String -> Line -> Model -> ( Model, List Effect )
lineIn raw l m =
    case l of
        Answer a ->
            ( answer a m, [] )

        Ok_ (Just g) ->
            ok (Just g) False (answered (\k -> k == AskedSince) m)

        -- `OK` alone is never a `since`'s: a set's, or another command's.
        Ok_ Nothing ->
            ok Nothing False (answered (\k -> k /= AskedSince) m)

        OkMore g ->
            ok (Just g) True (answered (\k -> k == AskedSince) m)

        Err_ why ->
            case m.stage of
                Following ->
                    -- A refused `since` stays owed; the timeout re-asks, then settles it as lost.
                    ( refusedCommand why m, [] )

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
            ( answered (\k -> k == AskedSet a) { m | refusals = Dict.insert (Addr.key a) why m.refusals }, [] )

        Queued ->
            ( answered (\k -> k /= AskedSince) m, [] )

        -- Asking again gets the same: stop, and say so.
        SinceTooLarge ->
            ( { m | stage = Halted TooLarge, asked = Nothing }, [] )

        Unknown ->
            ( unread raw m, [] )


{-| An `ERR` while following: the answer to the oldest command still
waiting, whatever it is. Kept with it; a set's is shown at its control too.
-}
refusedCommand : String -> Model -> Model
refusedCommand why m =
    let
        rest : Model
        rest =
            keep (Refusal { sent = List.head m.awaiting, why = why }) { m | awaiting = List.drop 1 m.awaiting }
    in
    case List.head m.awaiting |> Maybe.map askedBy of
        Just (AskedSet a) ->
            { rest | refusals = Dict.insert (Addr.key a) why rest.refusals }

        Just AskedSince ->
            rest

        Just AskedOther ->
            rest

        Nothing ->
            rest


refused : String -> Model -> Model
refused why m =
    { m | stage = Halted (RefusedBy why), asked = Nothing }


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
                describing a m

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
                describing a m

        Page _ ->
            describing a m

        Param _ ->
            describing a m

        Opt _ ->
            describing a m

        Route _ ->
            describing a m

        Src _ ->
            describing a m

        Col _ ->
            describing a m

        AmountRange _ _ ->
            describing a m

        Dests _ ->
            describing a m

        RoutesIs ->
            { m | routes = Dict.empty }

        RouteIs r ->
            { m | routes = Dict.insert (cellKey r.cell) { amount = r.amount, text = r.text } m.routes }

        Unroute c ->
            { m | routes = Dict.remove (cellKey c) m.routes }

        Crumb _ ->
            describing a m

        -- SETTINGS' section whole: its state goes. Its values are THEME's,
        -- MIDI CONFIG's and RANDOMIZE's, a fixed set the whole answer
        -- repeats; a Part's SETTINGS cells are the mixer's section's.
        SettingsIs ->
            { m | project = Nothing, sounds = Dict.empty, toast = Nothing, listing = Nothing }

        ProjectIs p ->
            { m | project = Just p }

        SoundIs snd ->
            { m | sounds = Dict.insert snd.part { name = snd.name, status = snd.status } m.sounds }

        ToastIs t ->
            { m | toast = Just t }

        -- The listing whole: what was listed before is gone.
        ProjectsIs r ->
            { m | listing = Just { projects = [], more = r.more } }

        Proj pr ->
            { m | listing = m.listing |> Maybe.map (\l -> { l | projects = l.projects ++ [ pr ] }) }

        InputIs _ ->
            describing a m

        KindIs _ ->
            describing a m

        CcsAre _ ->
            describing a m

        MsbsAre _ ->
            describing a m

        SpanIs _ _ ->
            describing a m

        RowsAre _ ->
            describing a m

        MapRowsAre _ ->
            describing a m

        -- MAP's section whole: it repeats its rows, learn and the listing,
        -- so what was held for it goes.
        MapIs h ->
            { m | mapHeader = Just h, soundRows = Dict.empty, mapRows = Dict.empty, learn = Nothing, mapFiles = [] }

        RowIs r ->
            let
                reading : MapRowReading
                reading =
                    { input = r.input, kind = r.kind, addr = r.addr, min = r.min, max = r.max, echo = r.echo, status = r.status }
            in
            case r.row of
                SoundRow i ->
                    { m | soundRows = Dict.insert (rowIndexToInt i) reading m.soundRows }

                MapFileRow i ->
                    { m | mapRows = Dict.insert (rowIndexToInt i) reading m.mapRows }

        -- Whole or a diff's only line: it replaces the learn held.
        LearnIs l ->
            { m | learn = Just l }

        MapFileIs name ->
            { m | mapFiles = m.mapFiles ++ [ name ] }

        Act _ ->
            describing a m

        -- ORBIT's section whole: its state and values go; it repeats them.
        OrbitIs ->
            { m
                | values = Dict.filter (\k _ -> not (isOrbitKey k m)) m.values
                , song = Nothing
                , slots = Dict.empty
                , chords = Dict.empty
                , scale = Nothing
                , steps = Dict.empty
                , pools = Dict.empty
            }

        -- Whole or a diff, each line replaces its own entry.
        SongIs g ->
            { m | song = Just g }

        SlotIs sl ->
            { m | slots = Dict.insert (sceneNoToInt sl.slot) { state = sl.state, rev = sl.rev } m.slots }

        ChordIs sg d ->
            { m | chords = Dict.insert (segmentToInt sg) d m.chords }

        ScaleIs mask ->
            { m | scale = Just mask }

        StepIs r i n ->
            { m | steps = Dict.insert ( ringNoToInt r, stepNoToInt i ) n m.steps }

        PoolIs r ns ->
            { m | pools = Dict.insert (ringNoToInt r) ns m.pools }


{-| Only the MIXER's, SETTINGS' and ORBIT's values and the MIXER's panels
kept: the Part's section starts afresh.
-}
mixerOnly : Model -> Model
mixerOnly m =
    { m
        | values = Dict.filter (\k _ -> isMixerKey k m || isSettingsKey k m || isOrbitKey k m) m.values
        , variants = Dict.filter (\k _ -> Schema.hasPanel k m.mixer) m.variants
    }


isMixerKey : String -> Model -> Bool
isMixerKey k m =
    Addr.fromWire k |> Maybe.map (\a -> isMixer a m) |> Maybe.withDefault False


isSettingsKey : String -> Model -> Bool
isSettingsKey k m =
    Addr.fromWire k |> Maybe.map (\a -> isSettings a m) |> Maybe.withDefault False


isOrbitKey : String -> Model -> Bool
isOrbitKey k m =
    Addr.fromWire k |> Maybe.map (\a -> isOrbit a m) |> Maybe.withDefault False


{-| A schema read this connection, or none.
-}
heldSchema : SchemaId -> Model -> Maybe Schema
heldSchema s m =
    Dict.get (schemaToken s) m.held |> Maybe.map .schema


{-| A line of `ed describe`'s answer, into the schema being read (`line`
keeps it as the unit wrote it, for the hash and the cache).
-}
describing : AnswerLine -> Model -> Model
describing a m =
    case m.stage of
        Describing _ _ _ ->
            { m | incoming = Schema.add a m.incoming }

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
ok : Maybe Gen -> Bool -> Model -> ( Model, List Effect )
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
                        recall s r m1

                Nothing ->
                    recall s r m1

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
                    lines : List String
                    lines =
                        List.reverse m.incomingLines

                    read_ : Hash
                    read_ =
                        Hash.ofLines lines
                in
                if read_ == r.hash then
                    ( readNext (install s { hash = r.hash, schema = m.incoming } { m1 | incoming = Schema.empty, incomingLines = [] })
                    , [ Keep (cacheKey s r.hash) lines ]
                    )

                else
                    -- Not the text the hash names: neither shown nor kept.
                    ( { m1 | stage = Halted (BadSchema s (WrongHash r.hash read_)) }, [] )

        Following ->
            case done of
                Just g ->
                    ( following g (max 0 (m.owed - 1)) more m, [] )

                Nothing ->
                    ( m, [] )

        Halted _ ->
            ( m, [] )


{-| A schema the editor doesn't hold: from the cache, or, with no pages
(ORBIT or MIDI MAP not in the build), empty at once.
-}
recall : SchemaId -> Reply -> Model -> ( Model, List Effect )
recall s r m =
    if r.pages == 0 then
        ( readNext (install s { hash = r.hash, schema = Schema.empty } m), [] )

    else
        ( { m | stage = Recalling s r }, [ Recall (cacheKey s r.hash) ] )


{-| A `since` answered. Once nothing is owed and no command sent waits for
its answer (its `OK` would be read as `ed schema`'s), a box on another engine
has its schemas read (held, cached or described), then is read from nothing;
the old page stays up meanwhile.
-}
following : Gen -> Int -> Bool -> Model -> Model
following g owed more m =
    if owed == 0 && not more && List.isEmpty m.awaiting && m.engine /= Nothing && m.engine /= m.described then
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


{-| The cache's answer for the schema being recalled. Lines that hash to
what the unit said are describe's own text: read as its answer is, and a line
of them the editor can't read marks the schema bad. Anything else is a miss:
describe it.
-}
recalled : { key : String, lines : Maybe (List String) } -> Model -> Model
recalled r m =
    case m.stage of
        Recalling s reply ->
            if r.key /= cacheKey s reply.hash then
                m

            else
                let
                    describe : Model
                    describe =
                        { m | stage = Describing s reply Protocol.firstPage, incoming = Schema.empty, incomingLines = [] }
                in
                case r.lines |> Maybe.map (\ls -> ( Hash.ofLines ls == reply.hash, List.foldl read (Ok Schema.empty) ls )) of
                    Just ( True, Ok sch ) ->
                        if Schema.pageCount sch == Just reply.pages && List.length (Schema.pages sch) == reply.pages then
                            readNext (install s { hash = reply.hash, schema = sch } m)

                        else
                            describe

                    Just ( True, Err l ) ->
                        { m | stage = Halted (BadSchema s (Unreadable l)) }

                    Just ( False, _ ) ->
                        describe

                    Nothing ->
                        describe

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


{-| One cached line into a schema, as `describe`'s answer is read; the first
line that is not describe's (it can't be read, or is an answer's end) is the
error.
-}
read : String -> Result String Schema -> Result String Schema
read raw acc =
    case acc of
        Ok sch ->
            case parse raw of
                Answer a ->
                    Ok (Schema.add a sch)

                Ok_ _ ->
                    Err raw

                OkMore _ ->
                    Err raw

                Queued ->
                    Err raw

                Err_ _ ->
                    Err raw

                SetRefused _ _ ->
                    Err raw

                SinceTooLarge ->
                    Err raw

                Unknown ->
                    Err raw

        Err _ ->
            acc


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

        SettingsSchema ->
            { m1 | settings = h.schema }

        MapSchema ->
            { m1 | midiMap = h.schema }

        OrbitSchema ->
            { m1 | orbit = h.schema }


{-| What to read next: the followed engine's schema, the mixer's, the
engine's matrix, SETTINGS', MAP's, then ORBIT's, each once a connection;
then following.
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

            else if lacks SettingsSchema then
                go (Checking SettingsSchema Nothing)

            else if lacks MapSchema then
                go (Checking MapSchema Nothing)

            else if lacks OrbitSchema then
                go (Checking OrbitSchema Nothing)

            else
                go Following

        Nothing ->
            go (Halted UpdateEditor)
