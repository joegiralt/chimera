module ProtocolTest exposing (suite)

import Addr
import Expect
import Fixture
import FixtureData
import Hash
import Protocol exposing (ActCell(..), AddInput(..), AnswerLine(..), Cell, Clock(..), Command(..), Edits(..), Flags, Health(..), Kind(..), Learn(..), Line(..), MapCommand(..), MapForm(..), MapInput(..), MapKind(..), MapRow(..), NotePool, OnOff(..), OrbitCommand(..), OverEdits(..), Playback(..), ProjectFile(..), RouteKind(..), RouteTo(..), RowIndex, RowSetting(..), RowStatus(..), SaveInto(..), Scale(..), SchemaId(..), SlotState(..), SoundStatus(..), SpanPlace, Value(..), encode, optIndex, parse)
import Schema
import Test exposing (Test, describe, test)
import Unit
import Wire exposing (addr, page)


p : String -> String -> Line -> Test
p name raw want =
    test name <| \_ -> Expect.equal want (parse raw)


{-| A line that names a cell: `Unknown` if the text were no cell, so the
test fails on it.
-}
withCell : String -> (Cell -> AnswerLine) -> Line
withCell s f =
    Protocol.cell s |> Maybe.map (f >> Answer) |> Maybe.withDefault Unknown


{-| A since answer's number as `OK <m>` says it.
-}
okAt : Int -> Line
okAt n =
    Unit.genFromInt n |> Maybe.map (Just >> Ok_) |> Maybe.withDefault Unknown


none : Flags
none =
    { mappable = False, modulation = False }


{-| Option `n` as a set's value; a negative `n` is no option at all.
-}
idx : Int -> Value
idx n =
    case Protocol.optIndex n of
        Just i ->
            Index i

        Nothing ->
            Name "no option"


suite : Test
suite =
    describe "Protocol"
        [ describe "answers, copied from chimera-core/tests/console_ed_test.rs and the fixture"
            [ p "hello" "ed hello 4 0.3.0-dev part 1 ALGO" (Answer (HelloIs { protocol = 4, build = "0.3.0-dev", part = 1, engine = "ALGO" }))
            , p "page" "page 22 FLT of 36" (Protocol.pageLabelFrom "FLT" |> Maybe.map (\l -> Answer (Page { n = page 22, label = l, count = 36 })) |> Maybe.withDefault Unknown)
            , p "panel fixed" "panel ALG -" (Answer (Panel { name = "ALG", variant = "-" }))
            , p "panel variant" "panel FILTER CLEAN" (Answer (Panel { name = "FILTER", variant = "CLEAN" }))
            , p "param choice" "param ALGO.ALG_A \"ALG A\" choice lin 0 31 1 0 m" (Answer (Param { addr = addr "ALGO.ALG_A", label = "ALG A", kind = Choice, scale = Linear, min = 0, max = 31, step = 1, default = 0, flags = { mappable = True, modulation = False } }))
            , p "param cont" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625 -" (Answer (Param { addr = addr "OUT.LEVEL", label = "VOL", kind = Cont, scale = Linear, min = 0, max = 1, step = 0.0078125, default = 0.3515625, flags = none }))
            , p "param stepped" "param DELAY.NOTE \"TIME\" stepped lin 0 1 0.1 0.9 m" (Answer (Param { addr = addr "DELAY.NOTE", label = "TIME", kind = Stepped, scale = Linear, min = 0, max = 1, step = 0.1, default = 0.9, flags = { mappable = True, modulation = False } }))
            , p "param negative" "param ALGO.TRNSP \"TRNSP\" cont lin -24 24 1 0 d" (Answer (Param { addr = addr "ALGO.TRNSP", label = "TRNSP", kind = Cont, scale = Linear, min = -24, max = 24, step = 1, default = 0, flags = { mappable = False, modulation = True } }))
            , p "param octaves" "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000 md" (Answer (Param { addr = addr "FILTER.CUTOFF", label = "CUTOFF", kind = Cont, scale = Octaves, min = 20, max = 20000, step = 1, default = 1000, flags = { mappable = True, modulation = True } }))
            , p "a stored value that is no number is no val line (#1039)" "val FILTER.CUTOFF 0.5 nan \"?\"" Unknown
            , p "nor an infinity" "val FILTER.CUTOFF 0.5 -inf \"?\"" Unknown
            , p "nor inf" "val FILTER.CUTOFF 0.5 inf \"?\"" Unknown
            , p "a protocol 2 param line is not understood" "param OUT.LEVEL \"VOL\" cont 0 1 0.0078125 0.3515625" Unknown
            , p "a protocol 3 param line, no flags, is not understood" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625" Unknown
            , p "bad flags" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625 dm" Unknown
            , test "schema reads its schema, pages and hash" <|
                \_ ->
                    case parse "schema DRUM 165 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages, Hash.toHex r.hash ) ( EngineSchema Unit.Drum, 165, "0123456789abcdef" )

                        _ ->
                            Expect.fail "not a schema line"
            , p "schema with a short hash" "schema MIXER 31 00ff" Unknown
            , p "schema with an upper-case hash" "schema MIXER 31 00FF00FF00FF00FF" Unknown
            , p "schema of an unknown schema" "schema PADS 19 00ff00ff00ff00ff" Unknown
            , p "mixer" "mixer" (Answer MixerIs)
            , p "panel reverb" "panel REVERB HALL" (Answer (Panel { name = "REVERB", variant = "HALL" }))
            , p "bad scale" "param A.B \"L\" cont log 0 1 1 0" Unknown
            , p "source" "source \"KICK 909\"" (Answer (Source "KICK 909"))
            , p "opt" "opt ALGO.ALG_A 8 \"A1\"" (optIndex 8 |> Maybe.map (\ix -> Answer (Opt { addr = addr "ALGO.ALG_A", index = ix, name = "A1" })) |> Maybe.withDefault Unknown)
            , p "a protocol 3 route line is not understood" "route \"ENV AMT\" next-slice" Unknown
            , p "route knob" "route \"ENV AMT\" ENV1:FILTER.CUTOFF route" (withCell "ENV1:FILTER.CUTOFF" (\c -> Route { label = "ENV AMT", cell = c, kind = KnobRoute }))
            , p "keyboard knob" "route \"KEYBOARD\" NOTE:FILTER.CUTOFF keybd" (withCell "NOTE:FILTER.CUTOFF" (\c -> Route { label = "KEYBOARD", cell = c, kind = KnobKeybd }))
            , p "jack knob" "route \"FREQ MOD\" FM3IN route" (withCell "FM3IN" (\c -> Route { label = "FREQ MOD", cell = c, kind = KnobRoute }))
            , p "routes" "routes 3" (Answer RoutesIs)
            , p "a route's amount" "route LFO1:OP_C.LEVEL -42 \"-42\"" (withCell "LFO1:OP_C.LEVEL" (\c -> RouteIs { cell = c, amount = -42, text = "-42" }))
            , p "a route's plus" "route ENV1:FILTER.CUTOFF 7 \"+7\"" (withCell "ENV1:FILTER.CUTOFF" (\c -> RouteIs { cell = c, amount = 7, text = "+7" }))
            , p "unroute" "unroute OP5:FILTER.RESO" (withCell "OP5:FILTER.RESO" Unroute)
            , p "src" "src OP5 \"OP 5\"" (Answer (Src { src = "OP5", label = "OP 5" }))
            , p "col" "col FREQ FILTER.CUTOFF" (Answer (Col { tag = "FREQ", addr = addr "FILTER.CUTOFF" }))
            , p "amount" "amount -127 127" (Answer (AmountRange -127 127))
            , p "dests" "dests 16" (Answer (Dests 16))
            , p "get val" "val OP_C.CRSE 0.0634921 4 \"1.00\"" (Answer (Val { addr = addr "OP_C.CRSE", norm = 0.0634921, stored = 4, text = "1.00" }))
            , p "val with unit" "val FILTER.CUTOFF 1 20000 \"20.0 kHz\"" (Answer (Val { addr = addr "FILTER.CUTOFF", norm = 1, stored = 20000, text = "20.0 kHz" }))
            , p "val text with a space and sign" "val PIT.NOTE 0.5 60 \"C4 +12ct\"" (Answer (Val { addr = addr "PIT.NOTE", norm = 0.5, stored = 60, text = "C4 +12ct" }))
            , p "val empty text" "val A.B 0 0 \"\"" (Answer (Val { addr = addr "A.B", norm = 0, stored = 0, text = "" }))
            , p "val inner quote not before a space" "val A.B 0 0 \"5\" wide\"" Unknown
            , p "val signed text" "val VOICEFX.MIX 1 1 \"+63\"" (Answer (Val { addr = addr "VOICEFX.MIX", norm = 1, stored = 1, text = "+63" }))
            , p "since part" "part 1 ALGO" (Answer (PartIs 1 "ALGO"))
            , p "since panel" "panel LFO3 CLASSIC" (Answer (Panel { name = "LFO3", variant = "CLASSIC" }))
            , p "since val" "val OUT.PAN 0.5 0 \"C\"" (Answer (Val { addr = addr "OUT.PAN", norm = 0.5, stored = 0, text = "C" }))
            , p "queued" "OK queued" Queued
            , p "refused" "refused PAD3.KNOB_G \"NOT ON THIS MACHINE\"" (Answer (Refused (addr "PAD3.KNOB_G") "NOT ON THIS MACHINE"))
            , p "refused with the formant budget's words" "refused OP_B.WAVE \"FORMANT BUDGET FULL\"" (Answer (Refused (addr "OP_B.WAVE") "FORMANT BUDGET FULL"))
            , p "refused, no address" "refused \"X\"" Unknown
            ]
        , test "every line of the unit's MIXER schema is understood" <|
            \_ ->
                FixtureData.mixer
                    |> List.filter (parse >> isUnknown)
                    |> Expect.equal []
        , test "the MIXER schema addresses each Part's mix and sends, and the buses, with flags" <|
            \_ ->
                let
                    params : List ( String, Flags )
                    params =
                        FixtureData.mixer
                            |> List.filterMap
                                (\l ->
                                    case parse l of
                                        Answer (Param q) ->
                                            Just ( Addr.key q.addr, q.flags )

                                        _ ->
                                            Nothing
                                )
                in
                Expect.all
                    [ \ps -> Expect.equal (Just { mappable = True, modulation = False }) (lookup "PART6.LEVEL" ps)
                    , \ps -> Expect.equal (Just none) (lookup "PART1.CH" ps)
                    , \ps -> Expect.notEqual Nothing (lookup "PART4_SND.SEND3" ps)
                    , \ps -> Expect.notEqual Nothing (lookup "COMP.THRESH" ps)
                    ]
                    params
        , describe "SETTINGS (slice 3)"
            [ p "crumb" "crumb \"MIDI\" \"CHANNELS\"" (Answer (Crumb { list = "MIDI", row = "CHANNELS" }))
            , p "settings" "settings" (Answer SettingsIs)
            , test "schema SETTINGS" <|
                \_ ->
                    case parse "schema SETTINGS 26 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages ) ( SettingsSchema, 26 )

                        _ ->
                            Expect.fail "not a schema line"
            , test "a new project's line" <|
                \_ ->
                    Expect.equal (Answer (ProjectIs { name = "NEW PROJECT", file = NewProject, edits = Edited, generation = Nothing }))
                        (parse "project \"NEW PROJECT\" new edited gen -")
            , test "a saved project's line" <|
                \_ ->
                    case parse "project \"MY SONG\" 12 clean gen 3" of
                        Answer (ProjectIs r) ->
                            Expect.equal ( r.name, ( r.file |> fileInt, r.edits ), r.generation |> Maybe.map Protocol.generationToInt ) ( "MY SONG", ( 12, Clean ), Just 3 )

                        _ ->
                            Expect.fail "not a project line"
            , p "a project line with a bad state" "project \"X\" 3 dirty gen 1" Unknown
            , p "a project line with id 0" "project \"X\" 0 clean gen 1" Unknown
            , p "sound" "sound 3 \"BASS 1\" stale" (Answer (SoundIs { part = 3, name = "BASS 1", status = SoundStale }))
            , p "sound edited" "sound 1 \"INIT\" edited" (Answer (SoundIs { part = 1, name = "INIT", status = SoundEdited }))
            , p "toast" "toast \"SAVED\"" (Answer (ToastIs "SAVED"))
            , p "projects" "projects 3" (Answer (ProjectsIs { count = 3, more = False }))
            , p "projects more" "projects 48 more" (Answer (ProjectsIs { count = 48, more = True }))
            , test "proj" <|
                \_ ->
                    case parse "proj 10 \"MY SONG\" ok gen 2" of
                        Answer (Proj r) ->
                            Expect.equal ( ( Protocol.projectIdToInt r.id, r.name ), r.health, r.generation |> Maybe.map Protocol.generationToInt ) ( ( 10, "MY SONG" ), FileOk, Just 2 )

                        _ ->
                            Expect.fail "not a proj line"
            , test "a damaged file" <|
                \_ ->
                    case parse "proj 4 \"\" bad gen -" of
                        Answer (Proj r) ->
                            Expect.equal ( r.health, r.generation ) ( FileBad, Nothing )

                        _ ->
                            Expect.fail "not a proj line"
            , p "a newer file" "proj 4 \"X\" weird gen 1" Unknown
            , test "every line of the unit's SETTINGS schema is understood" <|
                \_ ->
                    FixtureData.settings
                        |> List.filter (parse >> isUnknown)
                        |> Expect.equal []
            , test "every line of the unit's MAP schema is understood" <|
                \_ ->
                    FixtureData.map
                        |> List.filter (parse >> isUnknown)
                        |> Expect.equal []
            , test "every SETTINGS page names its crumbs" <|
                \_ ->
                    let
                        count : String -> Int
                        count head =
                            FixtureData.settings |> List.filter (String.startsWith head) |> List.length
                    in
                    Expect.equal (count "page ") (count "crumb ")
            , test "ask projects" <| \_ -> Expect.equal (Just "ed projects") (encode ListProjects)
            , test "save" <| \_ -> Expect.equal (Just "ed project save") (encode SaveProject)
            , test "saveas writes a space as _" <|
                \_ ->
                    Expect.equal (Just (Just "ed project saveas MY_SONG-2"))
                        (Protocol.projectName "MY SONG-2" |> Maybe.map (SaveProjectAs >> encode))
            , test "a name NAMING couldn't type is none" <|
                \_ ->
                    [ "", " A", "A ", "A_B", "A.B", "ABCDEFGHIJKLMNOPQ", "Ä" ]
                        |> List.map Protocol.projectName
                        |> Expect.equal (List.repeat 7 Nothing)
            , test "load" <|
                \_ ->
                    Expect.equal (Just [ Just "ed project load 10 3", Just "ed project load 10 3 discard" ])
                        (Maybe.map2 (\i g -> [ encode (LoadProject i g RefuseOverEdits), encode (LoadProject i g DiscardEdits) ]) (Protocol.projectId 10) (Protocol.generation 3))
            , test "a project id is 1 or more" <| \_ -> Expect.equal Nothing (Protocol.projectId 0)
            ]
        , describe "MAP (slice 4)"
            [ test "schema MAP" <|
                \_ ->
                    case parse "schema MAP 1 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages ) ( MapSchema, 1 )

                        _ ->
                            Expect.fail "not a schema line"
            , test "MAP's token" <| \_ -> Expect.equal ( Just "ed schema MAP", Just "ed describe MAP 0" ) ( encode (AskSchema MapSchema), encode (Describe MapSchema Protocol.firstPage) )
            , p "input CC" "input CC JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 MOMENTARY TOGGLE" (Answer (InputIs { form = FormCc, kinds = [ Jump, Pickup, ScaleTo, TwosComp, BinaryOffset, SignedBit, SignedBit2, Momentary, ToggleButton ] }))
            , p "input SYSEX" "input SYSEX JUMP" (Answer (InputIs { form = FormSysex, kinds = [ Jump ] }))
            , p "input of no form" "input MIDI JUMP" Unknown
            , p "input with no such kind" "input CC JUMP SOFT" Unknown
            , p "kind" "kind TWOS-COMP \"RELATIVE · TWOS COMPLEMENT\" \"+1 = 1  -1 = 127\"" (Answer (KindIs { kind = TwosComp, head = "RELATIVE · TWOS COMPLEMENT", tail = "+1 = 1  -1 = 127" }))
            , p "kind with no tail" "kind JUMP \"ABSOLUTE · JUMPS TO THE KNOB\" \"\"" (Answer (KindIs { kind = Jump, head = "ABSOLUTE · JUMPS TO THE KNOB", tail = "" }))
            , p "no such kind" "kind SOFT \"X\" \"\"" Unknown
            , pm "ccs" "ccs 1 2 3 74 127" (each Protocol.controlNo [ 1, 2, 3, 74, 127 ] |> Maybe.map (CcsAre >> Answer))
            , p "a CC past 127" "ccs 1 128" Unknown
            , pm "msbs" "msbs 1 2 5 7 31" (each Protocol.msb [ 1, 2, 5, 7, 31 ] |> Maybe.map (MsbsAre >> Answer))
            , p "6 is no MSB" "msbs 1 6" Unknown
            , p "32 is no MSB" "msbs 1 32" Unknown
            , pm "span" "span 0 255" (Maybe.map2 (\a b -> Answer (SpanIs a b)) (Protocol.spanPlace 0) (Protocol.spanPlace 255))
            , p "a span past 255" "span 0 256" Unknown
            , p "rows" "rows 16" (Answer (RowsAre 16))
            , p "maprows" "maprows 90" (Answer (MapRowsAre 90))
            , test "every line of MAP's schema is understood" <|
                \_ ->
                    mapSchemaLines
                        |> List.filter (parse >> isUnknown)
                        |> Expect.equal []
            , test "the kinds a form takes are the schema's" <|
                \_ ->
                    mapSchemaLines
                        |> List.filterMap
                            (\l ->
                                case parse l of
                                    Answer (InputIs r) ->
                                        Just ( r.kinds, Protocol.formKinds r.form )

                                    _ ->
                                        Nothing
                            )
                        |> List.filter (\( said, ours ) -> said /= ours)
                        |> Expect.equal []
            , pm "map" "map 2 \"MY MAP\" keep on" (Unit.partFromInt 2 |> Maybe.map (\pt -> Answer (MapIs { part = pt, name = Just "MY MAP", keep = On })))
            , pm "map none" "map 1 none keep off" (Unit.partFromInt 1 |> Maybe.map (\pt -> Answer (MapIs { part = pt, name = Nothing, keep = Off })))
            , pm "a map named none" "map 1 \"none\" keep off" (Unit.partFromInt 1 |> Maybe.map (\pt -> Answer (MapIs { part = pt, name = Just "none", keep = Off })))
            , p "map of Part 7" "map 7 none keep on" Unknown
            , p "map keep maybe" "map 1 none keep maybe" Unknown
            , pm "a Sound row" "row S1 CC74 JUMP FILTER.CUTOFF 0 255 off live" (row SoundRow 1 (cc 74) Jump (Just "FILTER.CUTOFF") ( 0, 255 ) Off Live)
            , pm "a map row, 14-bit, inverted" "row M3 CC14:2 SCALE PART1.LEVEL 255 0 on shadowed" (row MapFileRow 3 (Protocol.msb 2 |> Maybe.map InCc14) ScaleTo (Just "PART1.LEVEL") ( 255, 0 ) On Shadowed)
            , pm "an NRPN row" "row M1 NRPN:1234 INC-DEC REVERB.MIX 10 20 off dead" (row MapFileRow 1 (Protocol.nrpnNo 1234 |> Maybe.map InNrpn) IncDec (Just "REVERB.MIX") ( 10, 20 ) Off Dead)
            , pm "a row the unit can't name" "row S2 PRESS PICKUP - 0 255 off live" (row SoundRow 2 (Just InPress) Pickup Nothing ( 0, 255 ) Off Live)
            , pm "a SysEx row" "row M16 SYSEX JUMP OUT.SEND1 0 255 off live" (row MapFileRow 16 (Just InSysex) Jump (Just "OUT.SEND1") ( 0, 255 ) Off Live)
            , test "rows the unit never writes" <|
                \_ ->
                    [ "row X1 CC74 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S0 CC74 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S1 CC128 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S1 CC14:6 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S1 CC14:40 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S1 NRPN:16384 JUMP FILTER.CUTOFF 0 255 off live"
                    , "row S1 CC74 SOFT FILTER.CUTOFF 0 255 off live"
                    , "row S1 CC74 JUMP FILTER.CUTOFF 0 256 off live"
                    , "row S1 CC74 JUMP FILTER.CUTOFF 0 255 maybe live"
                    , "row S1 CC74 JUMP FILTER.CUTOFF 0 255 off alive"
                    , "row S1 CC74 JUMP FILTER.CUTOFF 0 255 off shadowed"
                    ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 11 True)
            , p "learn off" "learn off \"\"" (Answer (LearnIs { learn = NotLearning, note = "" }))
            , p "learn armed" "learn armed FILTER.CUTOFF \"MOVE A CONTROL\"" (Answer (LearnIs { learn = Armed (Just (addr "FILTER.CUTOFF")), note = "MOVE A CONTROL" }))
            , p "learn armed on no address" "learn armed - \"MOVE A CONTROL\"" (Answer (LearnIs { learn = Armed Nothing, note = "MOVE A CONTROL" }))
            , pm "learn sensing" "learn sensing S3 \"CC74 HEARD\"" (Protocol.rowIndex 3 |> Maybe.map (\i -> Answer (LearnIs { learn = Sensing i, note = "CC74 HEARD" })))
            , p "learn pointing" "learn pointing \"TURN A KNOB\"" (Answer (LearnIs { learn = Pointing, note = "TURN A KNOB" }))
            , p "learn sensing a map row" "learn sensing M3 \"X\"" Unknown
            , p "learn of no state" "learn maybe \"X\"" Unknown
            , p "mapfile" "mapfile \"KEYSTEP 37\"" (Answer (MapFileIs "KEYSTEP 37"))
            , pe "add" "ed map add CC74 JUMP FILTER.CUTOFF" (add (Protocol.controlNo 74 |> Maybe.map AddCc) Jump "FILTER.CUTOFF" Nothing)
            , pe "add with a span" "ed map add NRPN:1234 INC-DEC REVERB.MIX 0 127" (add (Protocol.nrpnNo 1234 |> Maybe.map AddNrpn) IncDec "REVERB.MIX" (Just ( 0, 127 )))
            , pe "add a 14-bit pair" "ed map add CC14:2 SCALE PART1.LEVEL" (add (Protocol.msb 2 |> Maybe.map AddCc14) ScaleTo "PART1.LEVEL" Nothing)
            , pe "add pressure" "ed map add PRESS PICKUP RING2.HITS 255 0" (add (Just AddPress) Pickup "RING2.HITS" (Just ( 255, 0 )))
            , test "a binding takes only the kinds its form takes" <|
                \_ ->
                    [ Protocol.msb 2 |> Maybe.map AddCc14 |> Maybe.andThen (\i -> Protocol.binding i TwosComp)
                    , Protocol.binding AddPress Momentary
                    , Protocol.controlNo 1 |> Maybe.map AddCc |> Maybe.andThen (\i -> Protocol.binding i IncDec)
                    , Protocol.msb 2 |> Maybe.map AddCc14 |> Maybe.andThen (\i -> Protocol.binding i ToggleButton)
                    ]
                        |> Expect.equal [ Nothing, Nothing, Nothing, Nothing ]
            , pe "set control" "ed map set S1 control 74" (Maybe.map2 (\i c -> MapCmd (SetRow (SoundRow i) (RowControl c))) (Protocol.rowIndex 1) (Protocol.controlNo 74))
            , pe "set min" "ed map set M2 min 10" (Maybe.map2 (\i v -> MapCmd (SetRow (MapFileRow i) (RowMin v))) (Protocol.rowIndex 2) (Protocol.spanPlace 10))
            , pe "set max" "ed map set M2 max 200" (Maybe.map2 (\i v -> MapCmd (SetRow (MapFileRow i) (RowMax v))) (Protocol.rowIndex 2) (Protocol.spanPlace 200))
            , pe "set input" "ed map set S16 input SIGNED-2" (Protocol.rowIndex 16 |> Maybe.map (\i -> MapCmd (SetRow (SoundRow i) (RowKind SignedBit2))))
            , pe "set echo" "ed map set M1 echo on" (Protocol.rowIndex 1 |> Maybe.map (\i -> MapCmd (SetRow (MapFileRow i) (RowEcho On))))
            , pe "rm" "ed map rm M2 CC14:2 PART1.LEVEL" (Maybe.map2 (\i m -> MapCmd (RemoveRow (MapFileRow i) (InCc14 m) (Just (addr "PART1.LEVEL")))) (Protocol.rowIndex 2) (Protocol.msb 2))
            , pe "rm a row the unit can't name" "ed map rm S1 SYSEX -" (Protocol.rowIndex 1 |> Maybe.map (\i -> MapCmd (RemoveRow (SoundRow i) InSysex Nothing)))
            , pe "pick" "ed map pick MY_MAP-2" (Protocol.mapName "MY MAP-2" |> Maybe.map (\n -> MapCmd (PickMap (Just n))))
            , test "the plain map commands" <|
                \_ ->
                    [ PickMap Nothing, KeepMap On, KeepMap Off, SaveMap Nothing, ListMaps, LearnAt (addr "FILTER.CUTOFF"), StopLearning ]
                        |> List.map (MapCmd >> encode)
                        |> Expect.equal (List.map Just [ "ed map pick none", "ed map keep on", "ed map keep off", "ed map save", "ed maps", "ed learn FILTER.CUTOFF", "ed learn off" ])
            , pe "save into a name" "ed map save KEYSTEP" (Protocol.mapName "KEYSTEP" |> Maybe.map (\n -> MapCmd (SaveMap (Just n))))
            , pe "rename" "ed map rename MY_KEYS" (Protocol.mapName "MY KEYS" |> Maybe.map (\n -> MapCmd (RenameMap n)))
            , test "a map name NAMING couldn't type is none" <|
                \_ ->
                    [ "", " A", "A ", "A_B", "A.B", "ABCDEFGHIJKLMNOPQ", "Ä" ]
                        |> List.map Protocol.mapName
                        |> Expect.equal (List.repeat 7 Nothing)
            , test "the numbers' ranges" <|
                \_ ->
                    [ Protocol.controlNo 128 == Nothing
                    , Protocol.controlNo -1 == Nothing
                    , Protocol.msb 0 == Nothing
                    , Protocol.msb 6 == Nothing
                    , Protocol.msb 32 == Nothing
                    , Protocol.nrpnNo 16384 == Nothing
                    , Protocol.nrpnNo 16383 /= Nothing
                    , Protocol.spanPlace 256 == Nothing
                    , Protocol.rowIndex 0 == Nothing
                    ]
                        |> Expect.equal (List.repeat 9 True)
            ]
        , describe "ORBIT (slice 5)"
            [ test "schema ORBIT" <|
                \_ ->
                    case parse "schema ORBIT 19 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages ) ( OrbitSchema, 19 )

                        _ ->
                            Expect.fail "not a schema line"
            , test "a build without ORBIT has no ORBIT pages" <|
                \_ ->
                    case parse "schema ORBIT 0 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages ) ( OrbitSchema, 0 )

                        _ ->
                            Expect.fail "not a schema line"
            , test "ORBIT's token" <| \_ -> Expect.equal ( Just "ed schema ORBIT", Just "ed describe ORBIT 18" ) ( encode (AskSchema OrbitSchema), encode (Describe OrbitSchema (page 18)) )
            , p "act" "act \"IN SCALE\" SET" (Answer (Act { label = "IN SCALE", cell = ActSet }))
            , test "every act cell" <|
                \_ ->
                    [ "RING", "SCENE", "SAVE", "SWITCH", "DEGREE", "PLAY", "CURSOR", "SET", "NOTE", "LEARN", "CLEAR" ]
                        |> List.map (\c -> parse ("act \"X\" " ++ c))
                        |> Expect.equal
                            (List.map (\c -> Answer (Act { label = "X", cell = c }))
                                [ ActRing, ActScene, ActSave, ActSwitch, ActDegree, ActPlay, ActCursor, ActSet, ActNote, ActLearn, ActClear ]
                            )
            , p "an act of no cell" "act \"X\" BOUNCE" Unknown
            , p "an act with no label" "act RING" Unknown
            , test "every line of the unit's ORBIT schema is understood" <|
                \_ ->
                    FixtureData.orbit
                        |> List.filter (parse >> isUnknown)
                        |> Expect.equal []
            , test "the ORBIT schema's pages, in order" <|
                \_ ->
                    Expect.equal
                        ( Schema.pageCount Fixture.orbit
                        , Schema.pages Fixture.orbit |> List.map (.label >> Protocol.pageLabel)
                        )
                        ( Just 19
                        , [ "ORB", "CUSTOM", "CHRD", "SCN" ]
                            ++ List.concatMap (\k -> List.map (\s -> "RING" ++ String.fromInt k ++ "_" ++ s) [ "RHYTHM", "VOICE", "NOTES" ]) (List.range 1 5)
                        )
            , test "the ORBIT schema's acts, each on its page" <|
                \_ ->
                    Schema.pages Fixture.orbit
                        |> List.map (.acts >> List.map .cell)
                        |> Expect.equal
                            ([ [ ActRing, ActScene ], [ ActCursor, ActSet ], [ ActDegree, ActPlay ], [ ActScene, ActSave, ActSwitch ] ]
                                ++ List.concat (List.repeat 5 [ [], [], [ ActLearn, ActClear, ActNote ] ])
                            )
            , test "the ORBIT schema's acts keep their labels" <|
                \_ ->
                    Schema.pages Fixture.orbit
                        |> List.take 2
                        |> List.concatMap (.acts >> List.map .label)
                        |> Expect.equal [ "RING", "SCENE", "NOTE", "IN SCALE" ]
            , test "the ORBIT schema's addresses" <|
                \_ ->
                    [ "ORBIT.KEY", "ORBIT.SCALE", "ORBIT.BARS", "ORBIT.SEGS", "TEMPO.BPM", "RING1.STEPS", "RING5.TARGET" ]
                        |> List.map (\a -> Schema.param (addr a) Fixture.orbit |> Maybe.map (.addr >> Addr.key))
                        |> Expect.equal (List.map Just [ "ORBIT.KEY", "ORBIT.SCALE", "ORBIT.BARS", "ORBIT.SEGS", "TEMPO.BPM", "RING1.STEPS", "RING5.TARGET" ])
            , p "orbit" "orbit" (Answer OrbitIs)
            , pm "song" "song playing clock INT scene 3 queued 5 edited yes" (Maybe.map2 (\s q -> Answer (SongIs { playback = Playing, clock = ClockInt, scene = s, queued = Just q, edits = Edited })) (Protocol.sceneNo 3) (Protocol.sceneNo 5))
            , pm "song stopped, nothing queued" "song stopped clock EXT scene 8 queued - edited no" (Protocol.sceneNo 8 |> Maybe.map (\s -> Answer (SongIs { playback = Stopped, clock = ClockExt, scene = s, queued = Nothing, edits = Clean })))
            , test "songs the unit never writes" <|
                \_ ->
                    [ "song playing clock INT scene 0 queued - edited no"
                    , "song playing clock INT scene 9 queued - edited no"
                    , "song playing clock INT scene 1 queued 9 edited no"
                    , "song paused clock INT scene 1 queued - edited no"
                    , "song playing clock MIDI scene 1 queued - edited no"
                    , "song playing clock INT scene 1 queued - edited maybe"
                    , "song playing clock INT scene 1 edited no"
                    ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 7 True)
            , pm "slot" "slot 2 saved rev 7" (Maybe.map2 (\s r -> Answer (SlotIs { slot = s, state = SlotSaved, rev = r })) (Protocol.sceneNo 2) (Protocol.rev 7))
            , pm "slot neutral" "slot 8 neutral rev 4294967295" (Maybe.map2 (\s r -> Answer (SlotIs { slot = s, state = SlotNeutral, rev = r })) (Protocol.sceneNo 8) (Protocol.rev 4294967295))
            , test "slots the unit never writes" <|
                \_ ->
                    [ "slot 0 saved rev 1", "slot 9 saved rev 1", "slot 1 saved rev 4294967296", "slot 1 empty rev 1", "slot 1 saved 1" ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 5 True)
            , pm "chord" "chord 8 12" (Maybe.map2 (\s d -> Answer (ChordIs s d)) (Protocol.segment 8) (Protocol.degree 12))
            , test "chords the unit never writes" <|
                \_ ->
                    [ "chord 0 1", "chord 9 1", "chord 1 0", "chord 1 13", "chord 1 V" ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 5 True)
            , pm "scale" "scale 2741" (Protocol.scaleMask 2741 |> Maybe.map (ScaleIs >> Answer))
            , test "scales the unit never writes" <|
                \_ ->
                    [ "scale 0", "scale 2740", "scale 4096", "scale -1", "scale" ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 5 True)
            , pm "step" "step 5 32 127" (Maybe.map3 (\r i n -> Answer (StepIs r i n)) (Protocol.ringNo 5) (Protocol.stepNo 32) (Protocol.note 127))
            , test "steps the unit never writes" <|
                \_ ->
                    [ "step 0 1 60", "step 6 1 60", "step 1 0 60", "step 1 33 60", "step 1 1 128", "step 1 1" ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 6 True)
            , pm "pool" "pool 2 60 64 67" (Maybe.map2 (\r ns -> Answer (PoolIs r ns)) (Protocol.ringNo 2) (pool [ 60, 64, 67 ]))
            , pm "a pool of eight" "pool 1 0 1 2 3 4 5 6 127" (Maybe.map2 (\r ns -> Answer (PoolIs r ns)) (Protocol.ringNo 1) (pool [ 0, 1, 2, 3, 4, 5, 6, 127 ]))
            , test "pools the unit never writes" <|
                \_ ->
                    [ "pool 1", "pool 1 1 2 3 4 5 6 7 8 9", "pool 1 128", "pool 6 60", "pool 0 60" ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 5 True)
            , p "ORBIT's values are vals" "val RING3.HITS 0.25 8 \"8\"" (Answer (Val { addr = addr "RING3.HITS", norm = 0.25, stored = 8, text = "8" }))
            , test "play and stop" <| \_ -> Expect.equal [ Just "ed orbit play", Just "ed orbit stop" ] (List.map (OrbitCmd >> encode) [ PlaySong, StopSong ])
            , pe "queue" "ed scene queue 4" (Protocol.sceneNo 4 |> Maybe.map (QueueScene >> OrbitCmd))
            , pe "save into a neutral slot" "ed scene save 2" (Protocol.sceneNo 2 |> Maybe.map (\n -> OrbitCmd (SaveScene n IntoNeutral)))
            , pe "save over a saved slot" "ed scene save 2 over 7" (Maybe.map2 (\n r -> OrbitCmd (SaveScene n (Over r))) (Protocol.sceneNo 2) (Protocol.rev 7))
            , pe "clear" "ed scene clear 3 12" (Maybe.map2 (\n r -> OrbitCmd (ClearScene n r)) (Protocol.sceneNo 3) (Protocol.rev 12))
            , pe "reload" "ed scene reload 3 0" (Maybe.map2 (\n r -> OrbitCmd (ReloadScene n r)) (Protocol.sceneNo 3) (Protocol.rev 0))
            , pe "move" "ed scene move 1 4 9 4294967295" (Maybe.map4 (\a b ra rb -> Protocol.sceneMove ( a, ra ) ( b, rb ) |> Maybe.map (MoveScene >> OrbitCmd)) (Protocol.sceneNo 1) (Protocol.sceneNo 4) (Protocol.rev 9) (Protocol.rev 4294967295) |> Maybe.andThen identity)
            , test "a scene moves only to another slot" <|
                \_ ->
                    Expect.equal (Just Nothing) (Maybe.map2 (\a r -> Protocol.sceneMove ( a, r ) ( a, r )) (Protocol.sceneNo 3) (Protocol.rev 1))
            , pe "ed chord" "ed chord 8 12" (Maybe.map2 (\s d -> OrbitCmd (SetChord s d)) (Protocol.segment 8) (Protocol.degree 12))
            , pe "ed scale" "ed scale 2741" (Protocol.scaleMask 2741 |> Maybe.map (SetScale >> OrbitCmd))
            , pe "ed step" "ed step 2 16 60" (Maybe.map3 (\r i n -> OrbitCmd (SetStep r i n)) (Protocol.ringNo 2) (Protocol.stepNo 16) (Protocol.note 60))
            , pe "ed pool" "ed pool 5 60 64 67" (Maybe.map2 (\r ns -> OrbitCmd (SetPool r ns)) (Protocol.ringNo 5) (pool [ 60, 64, 67 ]))
            , test "ORBIT's numbers' ranges" <|
                \_ ->
                    [ Protocol.sceneNo 0 == Nothing
                    , Protocol.sceneNo 9 == Nothing
                    , Protocol.rev -1 == Nothing
                    , Protocol.rev 4294967296 == Nothing
                    , Protocol.segment 0 == Nothing
                    , Protocol.segment 9 == Nothing
                    , Protocol.degree 0 == Nothing
                    , Protocol.degree 13 == Nothing
                    , Protocol.scaleMask 0 == Nothing
                    , Protocol.scaleMask 2 == Nothing
                    , Protocol.scaleMask 4096 == Nothing
                    , Protocol.scaleMask 1 /= Nothing
                    , Protocol.scaleMask 4095 /= Nothing
                    , Protocol.ringNo 0 == Nothing
                    , Protocol.ringNo 6 == Nothing
                    , Protocol.stepNo 0 == Nothing
                    , Protocol.stepNo 33 == Nothing
                    , Protocol.note -1 == Nothing
                    , Protocol.note 128 == Nothing
                    , Protocol.notePool [] == Nothing
                    , pool (List.range 60 68) == Nothing
                    ]
                        |> Expect.equal (List.repeat 21 True)
            ]
        , describe "terminators"
            [ p "OK" "OK" (Ok_ Nothing)
            , p "OK m" "OK 2" (okAt 2)
            , p "OK big" "OK 999999999" (okAt 999999999)
            , p "OK 0 is no since answer: the unit counts from 1" "OK 0" Unknown
            , p "OK more" "OK 12 more" (Unit.genFromInt 12 |> Maybe.map OkMore |> Maybe.withDefault Unknown)
            , p "OK 0 more" "OK 0 more" Unknown
            , p "OK more with no number" "OK more" Unknown
            , p "OK and a stray word" "OK 12 less" Unknown
            , p "ERR takes" "ERR ed takes hello, describe, get, set, since or part" (Err_ "ed takes hello, describe, get, set, since or part")
            , p "ERR get" "ERR ed get has no CHORUS.MODE here" (Err_ "ed get has no CHORUS.MODE here")
            , p "ERR part" "ERR ed part takes 1-6" (Err_ "ed part takes 1-6")
            , p "ERR busy" "ERR busy" (Err_ "busy")
            , p "ERR not in build" "ERR ed since is not in this build" (Err_ "ed since is not in this build")
            , p "ERR unterminated quote" "ERR ed set has no \"X here" (Err_ "ed set has no \"X here")
            , p "ERR set refused" "ERR ed set FILTER.KIND has no such option" (SetRefused (addr "FILTER.KIND") "has no such option")
            , p "ERR set takes a number" "ERR ed set OP_A.WAVE takes a number" (SetRefused (addr "OP_A.WAVE") "takes a number")
            , p "ERR set has no address here" "ERR ed set has no CHORUS.MODE here" (SetRefused (addr "CHORUS.MODE") "has no CHORUS.MODE here")
            , p "ERR set not in build" "ERR ed set is not in this build" (Err_ "ed set is not in this build")
            , p "ERR leading space" "  ERR busy" (Err_ "busy")
            , p "ERR alone" "ERR" (Err_ "")
            , p "CR stripped" "OK 3\u{000D}" (okAt 3)
            ]
        , describe "garbage is ignorable"
            [ p "empty" "" Unknown
            , p "noise" "\u{0000}�xx" Unknown
            , p "truncated val" "val OP_C.CRSE 0.06" Unknown
            , p "bad number" "val A.B x y \"t\"" Unknown
            , p "unterminated quote" "opt A.B 1 \"oops" Unknown
            , p "bad kind" "param A.B \"L\" weird lin 0 1 1 0" Unknown
            , p "bad OK" "OK soon" Unknown
            , p "lone keyword" "page" Unknown
            , test "never crashes" <|
                \_ ->
                    [ "val", "val \"", "\"", "param", "ed hello", "ed hello x", "OK -1", "part x", "route", "opt", "  " ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 11 True)
            ]
        , describe "commands"
            [ test "hello" <| \_ -> Expect.equal (Just "ed hello") (encode Hello)
            , test "describe" <| \_ -> Expect.equal (Just "ed describe ALGO 3") (encode (Describe (EngineSchema Unit.Algo) (page 3)))
            , test "describe the mixer" <| \_ -> Expect.equal (Just "ed describe MIXER 30") (encode (Describe MixerSchema (page 30)))
            , test "ask a schema" <| \_ -> Expect.equal (Just "ed schema DRUM") (encode (AskSchema (EngineSchema Unit.Drum)))
            , test "ask a matrix" <| \_ -> Expect.equal (Just "ed describe MTX-VA 0") (encode (Describe (MatrixSchema Unit.Va) Protocol.firstPage))
            , test "route an amount" <|
                \_ ->
                    Expect.equal (Just (Just "ed route LFO1:FILTER.CUTOFF -42"))
                        (Maybe.map2 (\c a -> encode (SetRoute c (RouteAmount a))) (Protocol.cell "LFO1:FILTER.CUTOFF") (Protocol.amount -42))
            , test "route off" <| \_ -> Expect.equal (Just (Just "ed route QMIN off")) (Protocol.cell "QMIN" |> Maybe.map (\c -> encode (SetRoute c RouteOff)))
            , test "an amount is -127..127" <| \_ -> Expect.equal [ Nothing, Nothing ] [ Protocol.amount 128, Protocol.amount -128 ]
            , test "text with a space, a control character or nothing is no cell" <|
                \_ ->
                    [ "OP 5:FILTER.CUTOFF", "QMIN\nkey b1", "" ]
                        |> List.map (Protocol.cell >> Maybe.map Protocol.cellKey)
                        |> Expect.equal [ Nothing, Nothing, Nothing ]
            , test "a cell is the unit's own word" <| \_ -> Expect.equal (Just "ENV1:FILTER.CUTOFF") (Protocol.cell "ENV1:FILTER.CUTOFF" |> Maybe.map Protocol.cellKey)
            , test "get" <| \_ -> Expect.equal (Just "ed get OP_C.CRSE") (encode (Get (addr "OP_C.CRSE")))
            , test "set num" <| \_ -> Expect.equal (Just "ed set FILTER.CUTOFF 1234.5") (encode (Set (addr "FILTER.CUTOFF") (Num 1234.5)))
            , test "set name" <| \_ -> Expect.equal (Just "ed set FILTER.KIND LADDER") (encode (Set (addr "FILTER.KIND") (Name "LADDER")))
            , test "set index" <| \_ -> Expect.equal (Just "ed set ENV1.HOLD #2") (encode (Set (addr "ENV1.HOLD") (idx 2)))
            , test "an option number is never negative" <| \_ -> Expect.equal Nothing (optIndex -1)
            , test "every engine the unit has, and no other, in its order" <|
                -- The firmware writes `EngineType::PLAYABLE`'s names to the fixture
                -- (console_ed_engines_test): a new engine there fails here
                -- until `Unit.allEngines` names it.
                \_ -> Expect.equal FixtureData.engines (List.map Unit.engineName Unit.allEngines)
            , test "every engine name reads back as its engine" <|
                \_ -> Expect.equal (List.map Just Unit.allEngines) (List.map Unit.engineFromName FixtureData.engines)
            , test "every engine is described by its name" <|
                \_ ->
                    Expect.equal (List.map (\e -> Just ("ed describe " ++ Unit.engineName e ++ " 0")) Unit.allEngines)
                        (List.map (\e -> encode (Describe (EngineSchema e) Protocol.firstPage)) Unit.allEngines)
            , p "since too large" "ERR ed since too large" SinceTooLarge
            , test "since" <| \_ -> Expect.equal (Just "ed since 7") (encode (Since (Unit.genFromInt 7)))
            , test "since nothing held is 0" <| \_ -> Expect.equal (Just "ed since 0") (encode (Since Nothing))
            , test "part" <| \_ -> Expect.equal (Just (Just "ed part 2")) (Unit.partFromInt 2 |> Maybe.map (Follow >> encode))
            , test "an_address_that_would_split_or_inject_is_no_address" <|
                \_ ->
                    [ "A.B\ned hello", "A B", "", "AB", ".B", "A.", "A.\"B" ]
                        |> List.map Addr.fromWire
                        |> Expect.equal (List.repeat 7 Nothing)
            , test "a_field_that_is_no_unit_word_makes_no_address" <|
                \_ ->
                    [ "A B", "", "cutoff", "X\n", ".B", "A.", "A\"B" ]
                        |> List.map (\w -> encode (Set (Addr.at Addr.Kit (Addr.field w)) (Num 1)))
                        |> Expect.equal (List.repeat 7 Nothing)
            , test "a_unit_word_makes_its_address" <|
                \_ ->
                    [ "TONAL", "E.DPT", "GATE_DECAY", "KNOB_G" ]
                        |> List.map (\w -> encode (Get (Addr.at Addr.Kit (Addr.field w))))
                        |> Expect.equal (List.map (\w -> Just ("ed get KIT." ++ w)) [ "TONAL", "E.DPT", "GATE_DECAY", "KNOB_G" ])
            , test "refuses a name that would split or inject" <|
                \_ ->
                    [ encode (Set (addr "A.B") (Name "X Y"))
                    , encode (Set (addr "A.B") (Name "X\u{000D}"))
                    ]
                        |> Expect.equal (List.repeat 2 Nothing)
            , test "refuses non-decimal floats" <|
                \_ ->
                    [ sqrt -1, 1 / 0, -1 / 0, 1.0e-7, 1.0e21 ]
                        |> List.map (\n -> encode (Set (addr "A.B") (Num n)))
                        |> Expect.equal (List.repeat 5 Nothing)
            ]
        ]


isUnknown : Line -> Bool
isUnknown l =
    case l of
        Unknown ->
            True

        Answer _ ->
            False

        SetRefused _ _ ->
            False

        Ok_ _ ->
            False

        OkMore _ ->
            False

        Queued ->
            False

        Err_ _ ->
            False

        SinceTooLarge ->
            False


fileInt : ProjectFile -> Int
fileInt f =
    case f of
        FileId i ->
            Protocol.projectIdToInt i

        NewProject ->
            0


lookup : String -> List ( String, a ) -> Maybe a
lookup k =
    List.filter (\( a, _ ) -> a == k) >> List.head >> Maybe.map Tuple.second


pm : String -> String -> Maybe Line -> Test
pm name raw want =
    test name <| \_ -> Expect.equal want (Just (parse raw))


{-| A command, made of smart constructors, encodes as `want`.
-}
pe : String -> String -> Maybe Command -> Test
pe name want c =
    test name <| \_ -> Expect.equal (Just (Just want)) (Maybe.map encode c)


each : (Int -> Maybe a) -> List Int -> Maybe (List a)
each f =
    List.foldr (\n acc -> Maybe.map2 (::) (f n) acc) (Just [])


{-| A pool of these notes, if each is a note and there are one to eight.
-}
pool : List Int -> Maybe NotePool
pool =
    each Protocol.note >> Maybe.andThen Protocol.notePool


cc : Int -> Maybe MapInput
cc =
    Protocol.controlNo >> Maybe.map InCc


row : (RowIndex -> MapRow) -> Int -> Maybe MapInput -> MapKind -> Maybe String -> ( Int, Int ) -> OnOff -> RowStatus -> Maybe Line
row which i input kind a ( lo, hi ) echo status =
    Maybe.map4 (\ix inp mn mx -> Answer (RowIs { row = which ix, input = inp, kind = kind, addr = Maybe.map addr a, min = mn, max = mx, echo = echo, status = status }))
        (Protocol.rowIndex i)
        input
        (Protocol.spanPlace lo)
        (Protocol.spanPlace hi)


add : Maybe AddInput -> MapKind -> String -> Maybe ( Int, Int ) -> Maybe Command
add input kind a span =
    let
        spanned : Maybe (Maybe ( SpanPlace, SpanPlace ))
        spanned =
            case span of
                Nothing ->
                    Just Nothing

                Just ( lo, hi ) ->
                    Maybe.map2 (\x y -> Just ( x, y )) (Protocol.spanPlace lo) (Protocol.spanPlace hi)
    in
    Maybe.map2 (\b s -> MapCmd (AddRow b (addr a) s)) (input |> Maybe.andThen (\i -> Protocol.binding i kind)) spanned


{-| MAP's schema as the unit describes it (ADR 0289, slice 4).
-}
mapSchemaLines : List String
mapSchemaLines =
    [ "page 0 MAP of 1"
    , "input CC JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 MOMENTARY TOGGLE"
    , "input CC14 JUMP PICKUP SCALE"
    , "input NRPN JUMP PICKUP SCALE TWOS-COMP OFFSET SIGNED SIGNED-2 INC-DEC MOMENTARY TOGGLE"
    , "input PRESS JUMP PICKUP SCALE"
    , "input SYSEX JUMP"
    , "kind JUMP \"ABSOLUTE · JUMPS TO THE KNOB\" \"\""
    , "kind PICKUP \"ABSOLUTE · WAITS TO PICK UP\" \"\""
    , "kind SCALE \"ABSOLUTE · SCALES TO MEET\" \"\""
    , "kind TWOS-COMP \"RELATIVE · TWOS COMPLEMENT\" \"+1 = 1  -1 = 127\""
    , "kind OFFSET \"RELATIVE · BINARY OFFSET\" \"+1 = 65  -1 = 63\""
    , "kind SIGNED \"RELATIVE · SIGNED BIT\" \"+1 = 1  -1 = 65\""
    , "kind SIGNED-2 \"RELATIVE · SIGNED BIT 2\" \"+1 = 65  -1 = 1\""
    , "kind INC-DEC \"RELATIVE · NRPN\" \"INCREMENT AND DECREMENT\""
    , "kind MOMENTARY \"BUTTON · HELD IS MAX\" \"\""
    , "kind TOGGLE \"BUTTON · EACH PRESS FLIPS\" \"\""
    , "ccs 1 2 3 4 5 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 74 127"
    , "msbs 1 2 3 4 5 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31"
    , "span 0 255"
    , "rows 16"
    , "maprows 16"
    ]
