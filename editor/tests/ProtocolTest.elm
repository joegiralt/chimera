module ProtocolTest exposing (suite)

import Expect
import FixtureData
import Protocol exposing (Addr(..), AnswerLine(..), Command(..), Kind(..), Line(..), Scale(..), Value(..), encode, optIndex, parse)
import Test exposing (Test, describe, test)
import Unit


p : String -> String -> Line -> Test
p name raw want =
    test name <| \_ -> Expect.equal want (parse raw)


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
            [ p "hello" "ed hello 3 0.3.0-dev part 1 ALGO" (Answer (HelloIs { protocol = 3, build = "0.3.0-dev", part = 1, engine = "ALGO" }))
            , p "page" "page 22 FLT of 36" (Answer (Page { n = 22, label = "FLT", count = 36 }))
            , p "panel fixed" "panel ALG -" (Answer (Panel { name = "ALG", variant = "-" }))
            , p "panel variant" "panel FILTER CLEAN" (Answer (Panel { name = "FILTER", variant = "CLEAN" }))
            , p "param choice" "param ALGO.ALG_A \"ALG A\" choice lin 0 31 1 0" (Answer (Param { addr = Addr "ALGO.ALG_A", label = "ALG A", kind = Choice, scale = Linear, min = 0, max = 31, step = 1, default = 0 }))
            , p "param cont" "param OUT.LEVEL \"VOL\" cont lin 0 1 0.0078125 0.3515625" (Answer (Param { addr = Addr "OUT.LEVEL", label = "VOL", kind = Cont, scale = Linear, min = 0, max = 1, step = 0.0078125, default = 0.3515625 }))
            , p "param negative" "param ALGO.TRNSP \"TRNSP\" cont lin -24 24 1 0" (Answer (Param { addr = Addr "ALGO.TRNSP", label = "TRNSP", kind = Cont, scale = Linear, min = -24, max = 24, step = 1, default = 0 }))
            , p "param octaves" "param FILTER.CUTOFF \"CUTOFF\" cont oct 20 20000 1 1000" (Answer (Param { addr = Addr "FILTER.CUTOFF", label = "CUTOFF", kind = Cont, scale = Octaves, min = 20, max = 20000, step = 1, default = 1000 }))
            , p "a protocol 2 param line is not understood" "param OUT.LEVEL \"VOL\" cont 0 1 0.0078125 0.3515625" Unknown
            , p "bad scale" "param A.B \"L\" cont log 0 1 1 0" Unknown
            , p "source" "source \"KICK 909\"" (Answer (Source "KICK 909"))
            , p "opt" "opt ALGO.ALG_A 8 \"A1\"" (optIndex 8 |> Maybe.map (\ix -> Answer (Opt { addr = Addr "ALGO.ALG_A", index = ix, name = "A1" })) |> Maybe.withDefault Unknown)
            , p "route" "route \"ENV AMT\" next-slice" (Answer (Route "ENV AMT"))
            , p "get val" "val OP_C.CRSE 0.0634921 4 \"1.00\"" (Answer (Val { addr = Addr "OP_C.CRSE", norm = 0.0634921, stored = 4, text = "1.00" }))
            , p "val with unit" "val FILTER.CUTOFF 1 20000 \"20.0 kHz\"" (Answer (Val { addr = Addr "FILTER.CUTOFF", norm = 1, stored = 20000, text = "20.0 kHz" }))
            , p "val text with a space and sign" "val PIT.NOTE 0.5 60 \"C4 +12ct\"" (Answer (Val { addr = Addr "PIT.NOTE", norm = 0.5, stored = 60, text = "C4 +12ct" }))
            , p "val empty text" "val A.B 0 0 \"\"" (Answer (Val { addr = Addr "A.B", norm = 0, stored = 0, text = "" }))
            , p "val inner quote not before a space" "val A.B 0 0 \"5\" wide\"" Unknown
            , p "val signed text" "val VOICEFX.MIX 1 1 \"+63\"" (Answer (Val { addr = Addr "VOICEFX.MIX", norm = 1, stored = 1, text = "+63" }))
            , p "since part" "part 1 ALGO" (Answer (PartIs 1 "ALGO"))
            , p "since panel" "panel LFO3 CLASSIC" (Answer (Panel { name = "LFO3", variant = "CLASSIC" }))
            , p "since val" "val OUT.PAN 0.5 0 \"C\"" (Answer (Val { addr = Addr "OUT.PAN", norm = 0.5, stored = 0, text = "C" }))
            , p "queued" "OK queued" Queued
            ]
        , describe "terminators"
            [ p "OK" "OK" (Ok_ Nothing)
            , p "OK m" "OK 2" (Ok_ (Just 2))
            , p "OK big" "OK 999999999" (Ok_ (Just 999999999))
            , p "ERR takes" "ERR ed takes hello, describe, get, set, since or part" (Err_ "ed takes hello, describe, get, set, since or part")
            , p "ERR get" "ERR ed get has no CHORUS.MODE here" (Err_ "ed get has no CHORUS.MODE here")
            , p "ERR part" "ERR ed part takes 1-6" (Err_ "ed part takes 1-6")
            , p "ERR busy" "ERR busy" (Err_ "busy")
            , p "ERR not in build" "ERR ed since is not in this build" (Err_ "ed since is not in this build")
            , p "ERR unterminated quote" "ERR ed set has no \"X here" (Err_ "ed set has no \"X here")
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
            , test "describe" <| \_ -> Expect.equal (Just "ed describe ALGO 3") (encode (Describe Unit.Algo 3))
            , test "get" <| \_ -> Expect.equal (Just "ed get OP_C.CRSE") (encode (Get (Addr "OP_C.CRSE")))
            , test "set num" <| \_ -> Expect.equal (Just "ed set FILTER.CUTOFF 1234.5") (encode (Set (Addr "FILTER.CUTOFF") (Num 1234.5)))
            , test "set name" <| \_ -> Expect.equal (Just "ed set FILTER.KIND LADDER") (encode (Set (Addr "FILTER.KIND") (Name "LADDER")))
            , test "set index" <| \_ -> Expect.equal (Just "ed set ENV1.HOLD #2") (encode (Set (Addr "ENV1.HOLD") (idx 2)))
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
                        (List.map (\e -> encode (Describe e 0)) Unit.allEngines)
            , p "since too large" "ERR ed since too large" SinceTooLarge
            , test "since" <| \_ -> Expect.equal (Just "ed since 7") (encode (Since (Unit.genFromInt 7)))
            , test "since nothing held is 0" <| \_ -> Expect.equal (Just "ed since 0") (encode (Since Nothing))
            , test "part" <| \_ -> Expect.equal (Just (Just "ed part 2")) (Unit.partFromInt 2 |> Maybe.map (Follow >> encode))
            , test "refuses what would split or inject" <|
                \_ ->
                    [ encode (Get (Addr "A.B\ned hello"))
                    , encode (Get (Addr "A B"))
                    , encode (Get (Addr ""))
                    , encode (Set (Addr "A.B") (Name "X Y"))
                    , encode (Set (Addr "A.B") (Name "X\u{000D}"))
                    ]
                        |> Expect.equal (List.repeat 5 Nothing)
            , test "refuses non-decimal floats" <|
                \_ ->
                    [ 0 / 0, 1 / 0, -1 / 0, 1.0e-7, 1.0e21 ]
                        |> List.map (\n -> encode (Set (Addr "A.B") (Num n)))
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

        Ok_ _ ->
            False

        Queued ->
            False

        Err_ _ ->
            False

        SinceTooLarge ->
            False
