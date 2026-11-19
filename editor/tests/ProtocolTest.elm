module ProtocolTest exposing (suite)

import Addr
import Expect
import FixtureData
import Protocol exposing (AnswerLine(..), Cell(..), Command(..), Edits(..), Flags, Health(..), Kind(..), Line(..), OverEdits(..), ProjectFile(..), RouteKind(..), RouteTo(..), Scale(..), SchemaId(..), SoundStatus(..), Value(..), encode, optIndex, parse)
import Test exposing (Test, describe, test)
import Unit
import Wire exposing (addr, page)


p : String -> String -> Line -> Test
p name raw want =
    test name <| \_ -> Expect.equal want (parse raw)


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
            , p "param negative" "param ALGO.TRNSP \"TRNSP\" cont lin -24 24 1 0 d" (Answer (Param { addr = addr "ALGO.TRNSP", label = "TRNSP", kind = Cont, scale = Linear, min = -24, max = 24, step = 1, default = 0, flags = { mappable = False, modulation = True } }))
            , p "param octaves" "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000 md" (Answer (Param { addr = addr "FILTER.CUTOFF", label = "CUTOFF", kind = Cont, scale = Octaves, min = 20, max = 20000, step = 1, default = 1000, flags = { mappable = True, modulation = True } }))
            , p "a protocol 2 param line is not understood" "param OUT.LEVEL \"VOL\" cont 0 1 0.0078125 0.3515625" Unknown
            , p "a protocol 3 param line, no flags, is not understood" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625" Unknown
            , p "bad flags" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625 dm" Unknown
            , test "schema reads its schema, pages and hash" <|
                \_ ->
                    case parse "schema DRUM 165 0123456789abcdef" of
                        Answer (SchemaIs r) ->
                            Expect.equal ( r.schema, r.pages, Protocol.hashText r.hash ) ( EngineSchema Unit.Drum, 165, "0123456789abcdef" )

                        _ ->
                            Expect.fail "not a schema line"
            , p "schema with a short hash" "schema MIXER 31 00ff" Unknown
            , p "schema with an upper-case hash" "schema MIXER 31 00FF00FF00FF00FF" Unknown
            , p "schema of an unknown schema" "schema ORBIT 19 00ff00ff00ff00ff" Unknown
            , p "mixer" "mixer" (Answer MixerIs)
            , p "panel reverb" "panel REVERB HALL" (Answer (Panel { name = "REVERB", variant = "HALL" }))
            , p "bad scale" "param A.B \"L\" cont log 0 1 1 0" Unknown
            , p "source" "source \"KICK 909\"" (Answer (Source "KICK 909"))
            , p "opt" "opt ALGO.ALG_A 8 \"A1\"" (optIndex 8 |> Maybe.map (\ix -> Answer (Opt { addr = addr "ALGO.ALG_A", index = ix, name = "A1" })) |> Maybe.withDefault Unknown)
            , p "a protocol 3 route line is not understood" "route \"ENV AMT\" next-slice" Unknown
            , p "route knob" "route \"ENV AMT\" ENV1:FILTER.CUTOFF route" (Answer (Route { label = "ENV AMT", cell = Cell "ENV1:FILTER.CUTOFF", kind = KnobRoute }))
            , p "keyboard knob" "route \"KEYBOARD\" NOTE:FILTER.CUTOFF keybd" (Answer (Route { label = "KEYBOARD", cell = Cell "NOTE:FILTER.CUTOFF", kind = KnobKeybd }))
            , p "jack knob" "route \"FREQ MOD\" FM3IN route" (Answer (Route { label = "FREQ MOD", cell = Cell "FM3IN", kind = KnobRoute }))
            , p "routes" "routes 3" (Answer RoutesIs)
            , p "a route's amount" "route LFO1:OP_C.LEVEL -42 \"-42\"" (Answer (RouteIs { cell = Cell "LFO1:OP_C.LEVEL", amount = -42, text = "-42" }))
            , p "a route's plus" "route ENV1:FILTER.CUTOFF 7 \"+7\"" (Answer (RouteIs { cell = Cell "ENV1:FILTER.CUTOFF", amount = 7, text = "+7" }))
            , p "unroute" "unroute OP5:FILTER.RESO" (Answer (Unroute (Cell "OP5:FILTER.RESO")))
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
        , describe "terminators"
            [ p "OK" "OK" (Ok_ Nothing)
            , p "OK m" "OK 2" (Ok_ (Just 2))
            , p "OK big" "OK 999999999" (Ok_ (Just 999999999))
            , p "OK more" "OK 12 more" (OkMore 12)
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
            , p "CR stripped" "OK 3\u{000D}" (Ok_ (Just 3))
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
                        (Protocol.amount -42 |> Maybe.map (\a -> encode (SetRoute (Cell "LFO1:FILTER.CUTOFF") (RouteAmount a))))
            , test "route off" <| \_ -> Expect.equal (Just "ed route QMIN off") (encode (SetRoute (Cell "QMIN") RouteOff))
            , test "an amount is -127..127" <| \_ -> Expect.equal [ Nothing, Nothing ] [ Protocol.amount 128, Protocol.amount -128 ]
            , test "a cell with a space is no command" <| \_ -> Expect.equal Nothing (encode (SetRoute (Cell "OP 5:FILTER.CUTOFF") RouteOff))
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
