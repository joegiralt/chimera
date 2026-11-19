module Protocol exposing
    ( AddInput(..)
    , Amount
    , AnswerLine(..)
    , Binding
    , Cell(..)
    , Command(..)
    , ControlNo
    , Edits(..)
    , Flags
    , Generation
    , Hash
    , Health(..)
    , Kind(..)
    , Learn(..)
    , Line(..)
    , MapCommand(..)
    , MapForm(..)
    , MapInput(..)
    , MapKind(..)
    , MapName
    , MapRow(..)
    , Msb
    , NrpnNo
    , OnOff(..)
    , OptIndex
    , OverEdits(..)
    , PageLabel
    , PageNo
    , ProjectFile(..)
    , ProjectId
    , ProjectName
    , RouteKind(..)
    , RouteTo(..)
    , RowIndex
    , RowSetting(..)
    , RowStatus(..)
    , Scale(..)
    , SchemaId(..)
    , SoundStatus(..)
    , SpanPlace
    , Value(..)
    , amount
    , binding
    , cellKey
    , controlNo
    , encode
    , firstOption
    , firstPage
    , formKinds
    , generation
    , generationToInt
    , hashText
    , mapName
    , msb
    , nextPage
    , nrpnNo
    , optIndex
    , pageLabel
    , pageLabelFrom
    , pageNo
    , pageNoToInt
    , parse
    , projectId
    , projectIdToInt
    , projectName
    , rowIndex
    , rowIndexToInt
    , schemaToken
    , spanPlace
    )

{-| The unit's text console, `ed` namespace: one command out, answer lines in.
Parsing never fails: a line that isn't understood is `Unknown` and is ignored.
-}

import Addr exposing (Addr)
import Unit exposing (Engine, Gen, Part)


{-| A page of `ed describe`, by the unit's number for it: 0 is the first.
-}
type PageNo
    = PageNo Int


pageNo : Int -> Maybe PageNo
pageNo n =
    if n >= 0 then
        Just (PageNo n)

    else
        Nothing


firstPage : PageNo
firstPage =
    PageNo 0


pageNoToInt : PageNo -> Int
pageNoToInt (PageNo n) =
    n


nextPage : PageNo -> PageNo
nextPage (PageNo n) =
    PageNo (n + 1)


{-| A page's name as the unit gives it (`FLT`, `PAD3_MCH`), read once.
-}
type PageLabel
    = PageLabel String


{-| A label off the wire: one word.
-}
pageLabelFrom : String -> Maybe PageLabel
pageLabelFrom s =
    word s |> Maybe.map PageLabel


pageLabel : PageLabel -> String
pageLabel (PageLabel l) =
    l


{-| A choice is sent by its option's number (`#2`), so a name with a space
(`GATE EXT`) is sent too. `Name` stays for a human's own words.
-}
type Value
    = Num Float
    | Name String
    | Index OptIndex


{-| A choice's option by its number, as `opt` lines give it: never
negative, so `#n` is always a number the unit reads.
-}
type OptIndex
    = OptIndex Int


{-| A choice's first option, `#0`.
-}
firstOption : OptIndex
firstOption =
    OptIndex 0


optIndex : Int -> Maybe OptIndex
optIndex i =
    if i >= 0 then
        Just (OptIndex i)

    else
        Nothing


{-| How a value moves under a hand, as the unit describes it.
-}
type Scale
    = Linear
    | Octaves


type Kind
    = Cont
    | Choice
    | Toggle


{-| What else may target a parameter, as the unit's rules say (ADR 0289):
a MIDI map (`m`), the mod matrix (`d`).
-}
type alias Flags =
    { mappable : Bool, modulation : Bool }


{-| A matrix cell, `ENV1:FILTER.CUTOFF`, or a jack knob's, `FM3IN`: the
unit's own text, opaque here.
-}
type Cell
    = Cell String


cellKey : Cell -> String
cellKey (Cell c) =
    c


{-| A route's amount as the box stores it: −127..127.
-}
type Amount
    = Amount Int


amount : Int -> Maybe Amount
amount n =
    if n >= -127 && n <= 127 then
        Just (Amount n)

    else
        Nothing


{-| What `ed route` sets a cell to: an amount, or no route (the grid's
MIX + MINUS).
-}
type RouteTo
    = RouteAmount Amount
    | RouteOff


{-| What a FLT › MOD knob's cell takes: any amount, or KEYBOARD's steps.
-}
type RouteKind
    = KnobRoute
    | KnobKeybd


{-| A schema the unit describes: an engine's pages, the mixer's, an
engine's mod matrix, SETTINGS' or MIDI MAP's.
-}
type SchemaId
    = EngineSchema Engine
    | MixerSchema
    | MatrixSchema Engine
    | SettingsSchema
    | MapSchema


{-| A project file's id on the card, as the listing names it: 1 or more.
-}
type ProjectId
    = ProjectId Int


projectId : Int -> Maybe ProjectId
projectId n =
    if n >= 1 then
        Just (ProjectId n)

    else
        Nothing


projectIdToInt : ProjectId -> Int
projectIdToInt (ProjectId n) =
    n


{-| A file's save generation as `since` said it: a load's witness.
-}
type Generation
    = Generation Int


generation : Int -> Maybe Generation
generation n =
    if n >= 0 then
        Just (Generation n)

    else
        Nothing


generationToInt : Generation -> Int
generationToInt (Generation n) =
    n


{-| A project's name as the box's NAMING types it: A–Z, a–z, 0–9, space and
`-`, one to sixteen of them, no space at either end.
-}
type ProjectName
    = ProjectName String


projectName : String -> Maybe ProjectName
projectName s =
    if naming s then
        Just (ProjectName s)

    else
        Nothing


{-| What the box's NAMING can type: A–Z, a–z, 0–9, space and `-`, one to
sixteen of them, no space at either end.
-}
naming : String -> Bool
naming s =
    let
        named : Char -> Bool
        named c =
            (Char.isAlphaNum c && Char.toCode c < 128) || c == ' ' || c == '-'
    in
    not (String.isEmpty s) && String.length s <= 16 && String.all named s && String.trim s == s


{-| A space on the wire is `_`, a byte NAMING never types.
-}
namedOnWire : String -> String
namedOnWire =
    String.replace " " "_"


{-| A load over a project with unsaved edits: refused, or the user said to
drop them (`discard`, after the editor's own confirm).
-}
type OverEdits
    = RefuseOverEdits
    | DiscardEdits


{-| The loaded project's file: its id, or a NEW project never saved.
-}
type ProjectFile
    = FileId ProjectId
    | NewProject


{-| Whether the loaded project has edits since its save or load.
-}
type Edits
    = Clean
    | Edited


{-| A Part's Sound against its slot, as the box's PART strip reads it.
-}
type SoundStatus
    = SoundClean
    | SoundEdited
    | SoundStale


{-| A listed file: loadable, damaged, or for newer firmware.
-}
type Health
    = FileOk
    | FileBad
    | FileNewer


{-| A MIDI MAP input's form, as MAP's schema lists them (`input <FORM> …`).
-}
type MapForm
    = FormCc
    | FormCc14
    | FormNrpn
    | FormPress
    | FormSysex


{-| How a row reads its control: the box's INPUT cell.
-}
type MapKind
    = Jump
    | Pickup
    | ScaleTo
    | TwosComp
    | BinaryOffset
    | SignedBit
    | SignedBit2
    | IncDec
    | Momentary
    | ToggleButton


{-| A control change number: 0..127.
-}
type ControlNo
    = ControlNo Int


controlNo : Int -> Maybe ControlNo
controlNo n =
    if n >= 0 && n <= 127 then
        Just (ControlNo n)

    else
        Nothing


{-| A 14-bit pair's MSB CC: 1..31 but 6 (its LSB is MSB + 32).
-}
type Msb
    = Msb Int


msb : Int -> Maybe Msb
msb n =
    if n >= 1 && n <= 31 && n /= 6 then
        Just (Msb n)

    else
        Nothing


{-| An NRPN number: 0..16383.
-}
type NrpnNo
    = NrpnNo Int


nrpnNo : Int -> Maybe NrpnNo
nrpnNo n =
    if n >= 0 && n <= 16383 then
        Just (NrpnNo n)

    else
        Nothing


{-| A place of a row's span, MIN or MAX: 0..255.
-}
type SpanPlace
    = SpanPlace Int


spanPlace : Int -> Maybe SpanPlace
spanPlace n =
    if n >= 0 && n <= 255 then
        Just (SpanPlace n)

    else
        Nothing


{-| A row's number, 1 or more: 1-based as the wire writes it.
-}
type RowIndex
    = RowIndex Int


rowIndex : Int -> Maybe RowIndex
rowIndex n =
    if n >= 1 then
        Just (RowIndex n)

    else
        Nothing


rowIndexToInt : RowIndex -> Int
rowIndexToInt (RowIndex n) =
    n


{-| A row of MIDI MAP: the Sound's own (`S<i>`) or the Part's controller
map's (`M<i>`).
-}
type MapRow
    = SoundRow RowIndex
    | MapFileRow RowIndex


{-| A row's control as `since` says it.
-}
type MapInput
    = InCc ControlNo
    | InCc14 Msb
    | InNrpn NrpnNo
    | InPress
    | InSysex


{-| A control `ed map add` binds: SysEx is learnt on the box, never added.
-}
type AddInput
    = AddCc ControlNo
    | AddCc14 Msb
    | AddNrpn NrpnNo
    | AddPress


{-| An input and a kind its form takes: only `binding` makes one.
-}
type Binding
    = Binding AddInput MapKind


binding : AddInput -> MapKind -> Maybe Binding
binding input kind =
    if List.member kind (formKinds (addForm input)) then
        Just (Binding input kind)

    else
        Nothing


addForm : AddInput -> MapForm
addForm input =
    case input of
        AddCc _ ->
            FormCc

        AddCc14 _ ->
            FormCc14

        AddNrpn _ ->
            FormNrpn

        AddPress ->
            FormPress


{-| The kinds a form takes, in the box's cycle order, as MAP's `input` lines
say: INC-DEC only on NRPN; relative kinds and buttons not on a 14-bit pair,
pressure or SysEx.
-}
formKinds : MapForm -> List MapKind
formKinds form =
    case form of
        FormCc ->
            [ Jump, Pickup, ScaleTo, TwosComp, BinaryOffset, SignedBit, SignedBit2, Momentary, ToggleButton ]

        FormCc14 ->
            [ Jump, Pickup, ScaleTo ]

        FormNrpn ->
            [ Jump, Pickup, ScaleTo, TwosComp, BinaryOffset, SignedBit, SignedBit2, IncDec, Momentary, ToggleButton ]

        FormPress ->
            [ Jump, Pickup, ScaleTo ]

        FormSysex ->
            [ Jump ]


{-| ECHO and KEEP MAP.
-}
type OnOff
    = On
    | Off


{-| A row's state: it drives its cell, its cell isn't here, or a Sound row
on the same control wins.
-}
type RowStatus
    = Live
    | Dead
    | Shadowed


{-| The box's MIDI learn: off, armed on a cell (or one it can't name),
hearing a control onto a Sound row, or MIDI MAP's pointing.
-}
type Learn
    = NotLearning
    | Armed (Maybe Addr)
    | Sensing RowIndex
    | Pointing


{-| A controller map's name as the box's NAMING types it.
-}
type MapName
    = MapName String


mapName : String -> Maybe MapName
mapName s =
    if naming s then
        Just (MapName s)

    else
        Nothing


{-| What `ed map set <row>` changes.
-}
type RowSetting
    = RowControl ControlNo
    | RowMin SpanPlace
    | RowMax SpanPlace
    | RowKind MapKind
    | RowEcho OnOff


{-| MIDI MAP's commands, and MIDI learn's.
-}
type MapCommand
    = AddRow Binding Addr (Maybe ( SpanPlace, SpanPlace ))
    | SetRow MapRow RowSetting
      -- Witnessed by the row's input and address as `since` said them.
    | RemoveRow MapRow MapInput (Maybe Addr)
      -- A map on the card, or none.
    | PickMap (Maybe MapName)
    | KeepMap OnOff
      -- Into the picked map, or into a named one the Part then picks.
    | SaveMap (Maybe MapName)
    | RenameMap MapName
      -- The box lists its maps; `mapfile` lines follow in `since`.
    | ListMaps
      -- The box goes to that cell's page, focuses it and arms learn.
    | LearnAt Addr
    | StopLearning


{-| A schema's hash, sixteen hex digits: describe's own bytes, so equal
hashes are the same text. Opaque: only `parse` makes one.
-}
type Hash
    = Hash String


hashText : Hash -> String
hashText (Hash h) =
    h


schemaToken : SchemaId -> String
schemaToken s =
    case s of
        EngineSchema e ->
            Unit.engineName e

        MixerSchema ->
            "MIXER"

        MatrixSchema e ->
            "MTX-" ++ Unit.engineName e

        SettingsSchema ->
            "SETTINGS"

        MapSchema ->
            "MAP"


schemaFromToken : String -> Maybe SchemaId
schemaFromToken t =
    if t == "MIXER" then
        Just MixerSchema

    else if t == "SETTINGS" then
        Just SettingsSchema

    else if t == "MAP" then
        Just MapSchema

    else if String.startsWith "MTX-" t then
        Unit.engineFromName (String.dropLeft 4 t) |> Maybe.map MatrixSchema

    else
        Unit.engineFromName t |> Maybe.map EngineSchema


type Command
    = Hello
    | AskSchema SchemaId
    | Describe SchemaId PageNo
    | Get Addr
    | Set Addr Value
    | Since (Maybe Gen)
    | Follow Part
    | SetRoute Cell RouteTo
      -- The card's projects listed again, for `since`'s listing.
    | ListProjects
      -- MENU hold's quick save, over the loaded project's own file.
    | SaveProject
      -- Into a new id, never over another project.
    | SaveProjectAs ProjectName
      -- A listed file, witnessed by its generation.
    | LoadProject ProjectId Generation OverEdits
    | MapCmd MapCommand


type AnswerLine
    = HelloIs { protocol : Int, build : String, part : Int, engine : String }
    | SchemaIs { schema : SchemaId, pages : Int, hash : Hash }
    | Page { n : PageNo, label : PageLabel, count : Int }
    | Panel { name : String, variant : String }
    | Param { addr : Addr, label : String, kind : Kind, scale : Scale, min : Float, max : Float, step : Float, default : Float, flags : Flags }
    | Opt { addr : Addr, index : OptIndex, name : String }
      -- FLT › MOD's knob: the matrix cell it views.
    | Route { label : String, cell : Cell, kind : RouteKind }
    | Source String
      -- MTX's schema: a source row, a fixed column, the amounts, the registry's columns.
    | Src { src : String, label : String }
    | Col { tag : String, addr : Addr }
    | AmountRange Int Int
    | Dests Int
      -- ROUTES: whole (`routes <n>`, n the followed Part's), a cell's amount
      -- and the box's text, a cell gone.
    | RoutesIs
    | RouteIs { cell : Cell, amount : Int, text : String }
    | Unroute Cell
    | Val { addr : Addr, norm : Float, stored : Float, text : String }
    | PartIs Int String
      -- The MIXER section of `since`, whole: what was held for it is dropped.
    | MixerIs
      -- A set the unit took and then would not apply, and the box's reason
      -- (its own toast's words).
    | Refused Addr String
      -- A SETTINGS page's place in the box's lists: its list's crumb and its own.
    | Crumb { list : String, row : String }
      -- The SETTINGS section of `since`, whole: what was held for it is dropped.
    | SettingsIs
    | ProjectIs { name : String, file : ProjectFile, edits : Edits, generation : Maybe Generation }
    | SoundIs { part : Int, name : String, status : SoundStatus }
      -- The box's toast, when a new one goes up.
    | ToastIs String
      -- The listing, whole: its count, and whether the card has more.
    | ProjectsIs { count : Int, more : Bool }
    | Proj { id : ProjectId, name : String, health : Health, generation : Maybe Generation }
      -- MAP's schema: a form and its kinds, a kind's box words, the CCs a
      -- binding may take, the 14-bit MSBs, a span's places, a Sound's rows
      -- and a controller map's.
    | InputIs { form : MapForm, kinds : List MapKind }
    | KindIs { kind : MapKind, head : String, tail : String }
    | CcsAre (List ControlNo)
    | MsbsAre (List Msb)
    | SpanIs SpanPlace SpanPlace
    | RowsAre Int
    | MapRowsAre Int
      -- The MAP section of `since`, whole: what was held for it is dropped.
    | MapIs { part : Part, name : Maybe String, keep : OnOff }
    | RowIs { row : MapRow, input : MapInput, kind : MapKind, addr : Maybe Addr, min : SpanPlace, max : SpanPlace, echo : OnOff, status : RowStatus }
    | LearnIs { learn : Learn, note : String }
    | MapFileIs String


type Line
    = Answer AnswerLine
    | Ok_ (Maybe Int)
      -- `OK <m> more`: a `since` answer with sections still owed; ask again.
    | OkMore Int
    | Queued
    | Err_ String
      -- `ERR ed set <addr> <why>`: a set the unit would not take.
    | SetRefused Addr String
      -- `ERR ed since too large` (ADR 0285): the unit's whole answer is past
      -- what one answer may carry. Asking again gets the same.
    | SinceTooLarge
    | Unknown


{-| The line to send, or `Nothing` if it could not be one command: an address
or name with whitespace or a control character would split or inject a second
command, and a float that is not a plain decimal (NaN, Infinity, `1.0e-7`)
is not one the unit reads.
-}
encode : Command -> Maybe String
encode c =
    case c of
        Hello ->
            Just "ed hello"

        AskSchema s ->
            Just ("ed schema " ++ schemaToken s)

        Describe s (PageNo page) ->
            Just ("ed describe " ++ schemaToken s ++ " " ++ String.fromInt page)

        Get a ->
            wire a |> Maybe.map (\k -> "ed get " ++ k)

        Set a v ->
            wire a |> Maybe.andThen (\k -> setValue v |> Maybe.map (\x -> "ed set " ++ k ++ " " ++ x))

        Since held ->
            Just ("ed since " ++ String.fromInt (held |> Maybe.map Unit.genToInt |> Maybe.withDefault 0))

        Follow p ->
            Just ("ed part " ++ String.fromInt (Unit.partNumber p))

        SetRoute (Cell cell) to ->
            word cell
                |> Maybe.map
                    (\x ->
                        "ed route "
                            ++ x
                            ++ " "
                            ++ (case to of
                                    RouteAmount (Amount n) ->
                                        String.fromInt n

                                    RouteOff ->
                                        "off"
                               )
                    )

        ListProjects ->
            Just "ed projects"

        SaveProject ->
            Just "ed project save"

        SaveProjectAs (ProjectName n) ->
            Just ("ed project saveas " ++ namedOnWire n)

        LoadProject (ProjectId id) (Generation g) over ->
            Just
                ("ed project load "
                    ++ String.fromInt id
                    ++ " "
                    ++ String.fromInt g
                    ++ (case over of
                            RefuseOverEdits ->
                                ""

                            DiscardEdits ->
                                " discard"
                       )
                )

        MapCmd mc ->
            encodeMap mc


encodeMap : MapCommand -> Maybe String
encodeMap c =
    case c of
        AddRow (Binding input kind) a span ->
            wire a
                |> Maybe.map
                    (\k ->
                        String.join " "
                            ([ "ed map add", inputText (addInput input), kindToken kind, k ]
                                ++ (case span of
                                        Just ( SpanPlace lo, SpanPlace hi ) ->
                                            [ String.fromInt lo, String.fromInt hi ]

                                        Nothing ->
                                            []
                                   )
                            )
                    )

        SetRow r setting ->
            Just (String.join " " [ "ed map set", rowText r, settingText setting ])

        RemoveRow r input a ->
            (case a of
                Just x ->
                    wire x

                Nothing ->
                    Just "-"
            )
                |> Maybe.map (\k -> String.join " " [ "ed map rm", rowText r, inputText input, k ])

        PickMap (Just (MapName n)) ->
            Just ("ed map pick " ++ namedOnWire n)

        PickMap Nothing ->
            Just "ed map pick none"

        KeepMap k ->
            Just ("ed map keep " ++ onOffText k)

        SaveMap (Just (MapName n)) ->
            Just ("ed map save " ++ namedOnWire n)

        SaveMap Nothing ->
            Just "ed map save"

        RenameMap (MapName n) ->
            Just ("ed map rename " ++ namedOnWire n)

        ListMaps ->
            Just "ed maps"

        LearnAt a ->
            wire a |> Maybe.map (\k -> "ed learn " ++ k)

        StopLearning ->
            Just "ed learn off"


addInput : AddInput -> MapInput
addInput input =
    case input of
        AddCc n ->
            InCc n

        AddCc14 m ->
            InCc14 m

        AddNrpn n ->
            InNrpn n

        AddPress ->
            InPress


inputText : MapInput -> String
inputText input =
    case input of
        InCc (ControlNo n) ->
            "CC" ++ String.fromInt n

        InCc14 (Msb n) ->
            "CC14:" ++ String.fromInt n

        InNrpn (NrpnNo n) ->
            "NRPN:" ++ String.fromInt n

        InPress ->
            "PRESS"

        InSysex ->
            "SYSEX"


rowText : MapRow -> String
rowText r =
    case r of
        SoundRow (RowIndex i) ->
            "S" ++ String.fromInt i

        MapFileRow (RowIndex i) ->
            "M" ++ String.fromInt i


settingText : RowSetting -> String
settingText setting =
    case setting of
        RowControl (ControlNo n) ->
            "control " ++ String.fromInt n

        RowMin (SpanPlace n) ->
            "min " ++ String.fromInt n

        RowMax (SpanPlace n) ->
            "max " ++ String.fromInt n

        RowKind k ->
            "input " ++ kindToken k

        RowEcho e ->
            "echo " ++ onOffText e


onOffText : OnOff -> String
onOffText o =
    case o of
        On ->
            "on"

        Off ->
            "off"


kindToken : MapKind -> String
kindToken k =
    case k of
        Jump ->
            "JUMP"

        Pickup ->
            "PICKUP"

        ScaleTo ->
            "SCALE"

        TwosComp ->
            "TWOS-COMP"

        BinaryOffset ->
            "OFFSET"

        SignedBit ->
            "SIGNED"

        SignedBit2 ->
            "SIGNED-2"

        IncDec ->
            "INC-DEC"

        Momentary ->
            "MOMENTARY"

        ToggleButton ->
            "TOGGLE"


{-| The address, if it is one the unit could write.
-}
wire : Addr -> Maybe String
wire a =
    if Addr.onWire a then
        Just (Addr.key a)

    else
        Nothing


setValue : Value -> Maybe String
setValue v =
    case v of
        Num n ->
            decimal n

        Name n ->
            word n

        Index (OptIndex i) ->
            Just ("#" ++ String.fromInt i)


word : String -> Maybe String
word w =
    if w /= "" && String.all (\ch -> Char.toCode ch > 32 && Char.toCode ch /= 127) w then
        Just w

    else
        Nothing


decimal : Float -> Maybe String
decimal n =
    let
        t : String
        t =
            String.fromFloat n
    in
    if isNaN n || isInfinite n || String.contains "e" t then
        Nothing

    else
        Just t


parse : String -> Line
parse raw =
    let
        line : String
        line =
            stripCr raw

        trimmed : String
        trimmed =
            String.trimLeft line
    in
    if trimmed == "ERR ed since too large" then
        SinceTooLarge

    else if String.startsWith "ERR ed set " trimmed then
        setRefusal (String.dropLeft 11 trimmed)

    else if trimmed == "ERR" || String.startsWith "ERR " trimmed then
        -- By prefix, not tokens: a refusal's text may hold a stray quote.
        Err_ (String.trim (String.dropLeft 3 trimmed))

    else if trimmed == "OK" || String.startsWith "OK " trimmed then
        case String.trim (String.dropLeft 2 trimmed) of
            "" ->
                Ok_ Nothing

            "queued" ->
                Queued

            m ->
                case String.split " " m of
                    [ n ] ->
                        nat n |> Maybe.map (Just >> Ok_) |> Maybe.withDefault Unknown

                    [ n, "more" ] ->
                        nat n |> Maybe.map OkMore |> Maybe.withDefault Unknown

                    _ ->
                        Unknown

    else
        case String.words trimmed of
            -- `none` bare is no map; a map named "none" is quoted, and the
            -- tokens drop the quotes.
            [ "map", n, "none", "keep", k ] ->
                mapHeader n Nothing k |> Maybe.map Answer |> Maybe.withDefault Unknown

            _ ->
                case tokens line of
                    Just ts ->
                        answer ts |> Maybe.map Answer |> Maybe.withDefault Unknown

                    Nothing ->
                        Unknown


{-| `<addr> <why>`, or `has no <addr> here`; anything else is a plain `ERR`.
-}
setRefusal : String -> Line
setRefusal rest =
    let
        plain : Line
        plain =
            Err_ ("ed set " ++ rest)
    in
    case String.words rest of
        "has" :: "no" :: a :: _ ->
            Addr.fromWire a |> Maybe.map (\x -> SetRefused x rest) |> Maybe.withDefault plain

        a :: ((_ :: _) as why) ->
            Addr.fromWire a |> Maybe.map (\x -> SetRefused x (String.join " " why)) |> Maybe.withDefault plain

        [ _ ] ->
            plain

        [] ->
            plain


stripCr : String -> String
stripCr s =
    if String.endsWith "\u{000D}" s then
        String.dropRight 1 s

    else
        s


nat : String -> Maybe Int
nat s =
    if s /= "" && String.all Char.isDigit s then
        String.toInt s

    else
        Nothing


num : String -> Maybe Float
num s =
    let
        body : String
        body =
            if String.startsWith "-" s || String.startsWith "+" s then
                String.dropLeft 1 s

            else
                s
    in
    -- String.toFloat takes "NaN", "Infinity" and "": the unit's numbers are plain decimals.
    if body /= "" && String.all (\c -> Char.isDigit c || c == '.') body then
        String.toFloat s

    else
        Nothing


answer : List String -> Maybe AnswerLine
answer ts =
    case ts of
        [ "ed", "hello", proto, build, "part", n, engine ] ->
            Maybe.map2 (\pr pt -> HelloIs { protocol = pr, build = build, part = pt, engine = engine }) (nat proto) (nat n)

        [ "schema", token, pages, hash ] ->
            Maybe.map3 (\sc n h -> SchemaIs { schema = sc, pages = n, hash = h }) (schemaFromToken token) (nat pages) (hashOf hash)

        [ "mixer" ] ->
            Just MixerIs

        [ "page", n, label, "of", count ] ->
            Maybe.map3 (\a l b -> Page { n = PageNo a, label = l, count = b }) (nat n) (pageLabelFrom label) (nat count)

        [ "panel", name, variant ] ->
            Just (Panel { name = name, variant = variant })

        [ "part", n, engine ] ->
            nat n |> Maybe.map (\i -> PartIs i engine)

        [ "param", a, label, kind, scale, lo, hi, step, def, flags ] ->
            Maybe.map2 Tuple.pair (scaleOf scale) (flagsOf flags)
                |> Maybe.andThen (\( sc, fl ) -> param a label kind sc fl ( lo, hi ) ( step, def ))

        [ "opt", a, i, name ] ->
            Maybe.map2 (\x ix -> Opt { addr = x, index = ix, name = name }) (Addr.fromWire a) (nat i |> Maybe.andThen optIndex)

        [ "route", a, b, c ] ->
            case int b of
                Just n ->
                    Just (RouteIs { cell = Cell a, amount = n, text = c })

                Nothing ->
                    routeKind c |> Maybe.map (\k -> Route { label = a, cell = Cell b, kind = k })

        [ "unroute", c ] ->
            Just (Unroute (Cell c))

        [ "routes", n ] ->
            nat n |> Maybe.map (always RoutesIs)

        [ "src", src, label ] ->
            Just (Src { src = src, label = label })

        [ "col", tag, a ] ->
            Addr.fromWire a |> Maybe.map (\x -> Col { tag = tag, addr = x })

        [ "amount", lo, hi ] ->
            Maybe.map2 AmountRange (int lo) (int hi)

        [ "dests", n ] ->
            nat n |> Maybe.map Dests

        [ "source", text ] ->
            Just (Source text)

        [ "val", a, norm, stored, text ] ->
            Maybe.map3 (\x n s -> Val { addr = x, norm = n, stored = s, text = text }) (Addr.fromWire a) (num norm) (num stored)

        [ "refused", a, why ] ->
            Addr.fromWire a |> Maybe.map (\x -> Refused x why)

        [ "crumb", list, row ] ->
            Just (Crumb { list = list, row = row })

        [ "settings" ] ->
            Just SettingsIs

        [ "project", name, file, edits, "gen", g ] ->
            Maybe.map3 (\f e gn -> ProjectIs { name = name, file = f, edits = e, generation = gn }) (fileOf file) (editsOf edits) (generationOf g)

        [ "sound", n, name, status ] ->
            Maybe.map2 (\i st -> SoundIs { part = i, name = name, status = st }) (nat n) (soundStatusOf status)

        [ "toast", text ] ->
            Just (ToastIs text)

        [ "projects", n ] ->
            nat n |> Maybe.map (\c -> ProjectsIs { count = c, more = False })

        [ "projects", n, "more" ] ->
            nat n |> Maybe.map (\c -> ProjectsIs { count = c, more = True })

        [ "proj", id, name, health, "gen", g ] ->
            Maybe.map3 (\i h gn -> Proj { id = i, name = name, health = h, generation = gn }) (nat id |> Maybe.andThen projectId) (healthOf health) (generationOf g)

        "input" :: form :: kinds ->
            Maybe.map2 (\f ks -> InputIs { form = f, kinds = ks }) (formOf form) (traverse mapKindOf kinds)

        [ "kind", k, head, tail ] ->
            mapKindOf k |> Maybe.map (\x -> KindIs { kind = x, head = head, tail = tail })

        "ccs" :: ns ->
            traverse (nat >> Maybe.andThen controlNo) ns |> Maybe.map CcsAre

        "msbs" :: ns ->
            traverse (nat >> Maybe.andThen msb) ns |> Maybe.map MsbsAre

        [ "span", lo, hi ] ->
            Maybe.map2 SpanIs (place lo) (place hi)

        [ "rows", n ] ->
            nat n |> Maybe.map RowsAre

        [ "maprows", n ] ->
            nat n |> Maybe.map MapRowsAre

        [ "map", n, name, "keep", k ] ->
            mapHeader n (Just name) k

        [ "row", r, input, kind, a, lo, hi, echo, status ] ->
            rowLine r input kind a ( lo, hi ) echo status

        [ "learn", "off", note ] ->
            Just (LearnIs { learn = NotLearning, note = note })

        [ "learn", "armed", a, note ] ->
            addrOrNone a |> Maybe.map (\x -> LearnIs { learn = Armed x, note = note })

        [ "learn", "sensing", r, note ] ->
            rowOf r
                |> Maybe.andThen
                    (\x ->
                        case x of
                            SoundRow i ->
                                Just (LearnIs { learn = Sensing i, note = note })

                            MapFileRow _ ->
                                Nothing
                    )

        [ "learn", "pointing", note ] ->
            Just (LearnIs { learn = Pointing, note = note })

        [ "mapfile", name ] ->
            Just (MapFileIs name)

        _ ->
            Nothing


{-| Every item read, or `Nothing`.
-}
traverse : (String -> Maybe a) -> List String -> Maybe (List a)
traverse f =
    List.foldr (\s acc -> Maybe.map2 (::) (f s) acc) (Just [])


place : String -> Maybe SpanPlace
place =
    nat >> Maybe.andThen spanPlace


mapHeader : String -> Maybe String -> String -> Maybe AnswerLine
mapHeader n name k =
    Maybe.map2 (\pt kp -> MapIs { part = pt, name = name, keep = kp }) (nat n |> Maybe.andThen Unit.partFromInt) (onOffOf k)


{-| A `row` line: S rows are only live or dead.
-}
rowLine : String -> String -> String -> String -> ( String, String ) -> String -> String -> Maybe AnswerLine
rowLine r input kind a ( lo, hi ) echo status =
    Maybe.map4
        (\( rw, st ) inp ( k, x ) ( mn, mx, e ) ->
            RowIs { row = rw, input = inp, kind = k, addr = x, min = mn, max = mx, echo = e, status = st }
        )
        (Maybe.map2 Tuple.pair (rowOf r) (rowStatusOf status)
            |> Maybe.andThen
                (\( rw, st ) ->
                    if statusFits rw st then
                        Just ( rw, st )

                    else
                        Nothing
                )
        )
        (inputOf input)
        (Maybe.map2 Tuple.pair (mapKindOf kind) (addrOrNone a))
        (Maybe.map3 (\mn mx e -> ( mn, mx, e )) (place lo) (place hi) (onOffOf echo))


{-| A Sound row is never shadowed: it is what shadows.
-}
statusFits : MapRow -> RowStatus -> Bool
statusFits rw st =
    case rw of
        SoundRow _ ->
            st /= Shadowed

        MapFileRow _ ->
            True


{-| An address, or `-` for one the unit can't name: `Just Nothing`.
-}
addrOrNone : String -> Maybe (Maybe Addr)
addrOrNone s =
    if s == "-" then
        Just Nothing

    else
        Addr.fromWire s |> Maybe.map Just


{-| `S<i>` or `M<i>`, 1-based.
-}
rowOf : String -> Maybe MapRow
rowOf s =
    case String.uncons s of
        Just ( 'S', i ) ->
            nat i |> Maybe.andThen rowIndex |> Maybe.map SoundRow

        Just ( 'M', i ) ->
            nat i |> Maybe.andThen rowIndex |> Maybe.map MapFileRow

        Just _ ->
            Nothing

        Nothing ->
            Nothing


{-| `CC74`, `CC14:2`, `NRPN:1234`, `PRESS` or `SYSEX`.
-}
inputOf : String -> Maybe MapInput
inputOf s =
    if s == "PRESS" then
        Just InPress

    else if s == "SYSEX" then
        Just InSysex

    else if String.startsWith "CC14:" s then
        nat (String.dropLeft 5 s) |> Maybe.andThen msb |> Maybe.map InCc14

    else if String.startsWith "NRPN:" s then
        nat (String.dropLeft 5 s) |> Maybe.andThen nrpnNo |> Maybe.map InNrpn

    else if String.startsWith "CC" s then
        nat (String.dropLeft 2 s) |> Maybe.andThen controlNo |> Maybe.map InCc

    else
        Nothing


formOf : String -> Maybe MapForm
formOf s =
    case s of
        "CC" ->
            Just FormCc

        "CC14" ->
            Just FormCc14

        "NRPN" ->
            Just FormNrpn

        "PRESS" ->
            Just FormPress

        "SYSEX" ->
            Just FormSysex

        _ ->
            Nothing


mapKindOf : String -> Maybe MapKind
mapKindOf s =
    case s of
        "JUMP" ->
            Just Jump

        "PICKUP" ->
            Just Pickup

        "SCALE" ->
            Just ScaleTo

        "TWOS-COMP" ->
            Just TwosComp

        "OFFSET" ->
            Just BinaryOffset

        "SIGNED" ->
            Just SignedBit

        "SIGNED-2" ->
            Just SignedBit2

        "INC-DEC" ->
            Just IncDec

        "MOMENTARY" ->
            Just Momentary

        "TOGGLE" ->
            Just ToggleButton

        _ ->
            Nothing


onOffOf : String -> Maybe OnOff
onOffOf s =
    case s of
        "on" ->
            Just On

        "off" ->
            Just Off

        _ ->
            Nothing


rowStatusOf : String -> Maybe RowStatus
rowStatusOf s =
    case s of
        "live" ->
            Just Live

        "dead" ->
            Just Dead

        "shadowed" ->
            Just Shadowed

        _ ->
            Nothing


{-| `<id>` or `new`.
-}
fileOf : String -> Maybe ProjectFile
fileOf s =
    if s == "new" then
        Just NewProject

    else
        nat s |> Maybe.andThen projectId |> Maybe.map FileId


editsOf : String -> Maybe Edits
editsOf s =
    case s of
        "clean" ->
            Just Clean

        "edited" ->
            Just Edited

        _ ->
            Nothing


soundStatusOf : String -> Maybe SoundStatus
soundStatusOf s =
    case s of
        "clean" ->
            Just SoundClean

        "edited" ->
            Just SoundEdited

        "stale" ->
            Just SoundStale

        _ ->
            Nothing


healthOf : String -> Maybe Health
healthOf s =
    case s of
        "ok" ->
            Just FileOk

        "bad" ->
            Just FileBad

        "newer" ->
            Just FileNewer

        _ ->
            Nothing


{-| A generation, or `-` for none: `Just Nothing`; anything else `Nothing`.
-}
generationOf : String -> Maybe (Maybe Generation)
generationOf s =
    if s == "-" then
        Just Nothing

    else
        nat s |> Maybe.map (Generation >> Just)


{-| A signed whole number: `-127`, `+42`, `42`.
-}
int : String -> Maybe Int
int s =
    case String.uncons s of
        Just ( '-', rest ) ->
            nat rest |> Maybe.map negate

        Just ( '+', rest ) ->
            nat rest

        Just _ ->
            nat s

        Nothing ->
            Nothing


routeKind : String -> Maybe RouteKind
routeKind s =
    case s of
        "route" ->
            Just KnobRoute

        "keybd" ->
            Just KnobKeybd

        _ ->
            Nothing


param : String -> String -> String -> Scale -> Flags -> ( String, String ) -> ( String, String ) -> Maybe AnswerLine
param a label kind scale flags ( lo, hi ) ( step, def ) =
    Maybe.map5
        (\( x, k ) l h s d -> Param { addr = x, label = label, kind = k, scale = scale, min = l, max = h, step = s, default = d, flags = flags })
        (Maybe.map2 Tuple.pair (Addr.fromWire a) (kindOf kind))
        (num lo)
        (num hi)
        (num step)
        (num def)


{-| `m`, `d`, `md` or `-`: nothing else.
-}
flagsOf : String -> Maybe Flags
flagsOf s =
    case s of
        "-" ->
            Just { mappable = False, modulation = False }

        "m" ->
            Just { mappable = True, modulation = False }

        "d" ->
            Just { mappable = False, modulation = True }

        "md" ->
            Just { mappable = True, modulation = True }

        _ ->
            Nothing


{-| Sixteen lowercase hex digits.
-}
hashOf : String -> Maybe Hash
hashOf h =
    if String.length h == 16 && String.all (\c -> Char.isDigit c || (c >= 'a' && c <= 'f')) h then
        Just (Hash h)

    else
        Nothing


scaleOf : String -> Maybe Scale
scaleOf s =
    case s of
        "lin" ->
            Just Linear

        "oct" ->
            Just Octaves

        _ ->
            Nothing


kindOf : String -> Maybe Kind
kindOf s =
    case s of
        "cont" ->
            Just Cont

        "choice" ->
            Just Choice

        "toggle" ->
            Just Toggle

        _ ->
            Nothing


{-| Split on spaces; a `"quoted"` run is one token, quotes dropped. A quote
closes only before a space or the end, so a quote inside a text survives.
`Nothing` for an unterminated quote.
-}
tokens : String -> Maybe (List String)
tokens s =
    go (String.toList s) [] |> Maybe.map List.reverse


go : List Char -> List String -> Maybe (List String)
go cs acc =
    case cs of
        [] ->
            Just acc

        ' ' :: rest ->
            go rest acc

        '"' :: rest ->
            quoted rest [] |> Maybe.andThen (\( t, after ) -> go after (t :: acc))

        _ ->
            let
                ( w, after ) =
                    spanWord cs []
            in
            go after (w :: acc)


spanWord : List Char -> List Char -> ( String, List Char )
spanWord cs w =
    case cs of
        [] ->
            ( String.fromList (List.reverse w), [] )

        ' ' :: _ ->
            ( String.fromList (List.reverse w), cs )

        c :: rest ->
            spanWord rest (c :: w)


quoted : List Char -> List Char -> Maybe ( String, List Char )
quoted cs w =
    case cs of
        [] ->
            Nothing

        '"' :: [] ->
            Just ( String.fromList (List.reverse w), [] )

        '"' :: ' ' :: rest ->
            Just ( String.fromList (List.reverse w), ' ' :: rest )

        c :: rest ->
            quoted rest (c :: w)
