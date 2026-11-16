module ProtocolTest exposing (suite)

import Expect
import Protocol exposing (..)
import Test exposing (Test, describe, test)


p : String -> String -> Line -> Test
p name raw want =
    test name <| \_ -> Expect.equal want (parse raw)


suite : Test
suite =
    describe "Protocol"
        [ describe "answers, copied from chimera-core/tests/console_ed_test.rs and the fixture"
            [ p "hello" "ed hello 2 0.3.0-dev part 1 ALGO" (Answer (HelloIs { protocol = 2, build = "0.3.0-dev", part = 1, engine = "ALGO" }))
            , p "page" "page 22 FLT of 36" (Answer (Page { n = 22, label = "FLT", count = 36 }))
            , p "panel fixed" "panel ALG -" (Answer (Panel { name = "ALG", variant = "-" }))
            , p "panel variant" "panel FILTER CLEAN" (Answer (Panel { name = "FILTER", variant = "CLEAN" }))
            , p "param choice" "param ALGO.ALG_A \"ALG A\" choice 0 31 1 0" (Answer (Param { addr = Addr "ALGO.ALG_A", label = "ALG A", kind = Choice, min = 0, max = 31, step = 1, default = 0 }))
            , p "param cont" "param OUT.LEVEL \"VOL\" cont 0 1 0.0078125 0.3515625" (Answer (Param { addr = Addr "OUT.LEVEL", label = "VOL", kind = Cont, min = 0, max = 1, step = 0.0078125, default = 0.3515625 }))
            , p "param negative" "param ALGO.TRNSP \"TRNSP\" cont -24 24 1 0" (Answer (Param { addr = Addr "ALGO.TRNSP", label = "TRNSP", kind = Cont, min = -24, max = 24, step = 1, default = 0 }))
            , p "opt" "opt ALGO.ALG_A 8 \"A1\"" (Answer (Opt { addr = Addr "ALGO.ALG_A", index = 8, name = "A1" }))
            , p "route" "route \"ENV AMT\" next-slice" (Answer (Route "ENV AMT"))
            , p "get val" "val OP_C.CRSE 0.0634921 4 \"1.00\"" (Answer (Val { addr = Addr "OP_C.CRSE", norm = 0.0634921, stored = 4, text = "1.00" }))
            , p "val with unit" "val FILTER.CUTOFF 1 20000 \"20.0 kHz\"" (Answer (Val { addr = Addr "FILTER.CUTOFF", norm = 1, stored = 20000, text = "20.0 kHz" }))
            , p "val signed text" "val VOICEFX.MIX 1 1 \"+63\"" (Answer (Val { addr = Addr "VOICEFX.MIX", norm = 1, stored = 1, text = "+63" }))
            , p "since part" "part 1 ALGO" (Answer (PartIs 1 "ALGO"))
            , p "since panel" "panel LFO3 CLASSIC" (Answer (Panel { name = "LFO3", variant = "CLASSIC" }))
            , p "since val" "val OUT.PAN 0.5 0 \"C\"" (Answer (Val { addr = Addr "OUT.PAN", norm = 0.5, stored = 0, text = "C" }))
            , p "queued" "OK queued" (Answer Queued)
            ]
        , describe "terminators"
            [ p "OK" "OK" (Ok_ Nothing)
            , p "OK m" "OK 2" (Ok_ (Just 2))
            , p "OK big" "OK 999999999" (Ok_ (Just 999999999))
            , p "ERR takes" "ERR ed takes hello, describe, get, set or since" (Err_ "ed takes hello, describe, get, set or since")
            , p "ERR get" "ERR ed get has no CHORUS.MODE here" (Err_ "ed get has no CHORUS.MODE here")
            , p "ERR busy" "ERR busy" (Err_ "busy")
            , p "ERR not in build" "ERR ed since is not in this build" (Err_ "ed since is not in this build")
            , p "CR stripped" "OK 3\u{000D}" (Ok_ (Just 3))
            ]
        , describe "garbage is ignorable"
            [ p "empty" "" (Unknown "")
            , p "noise" "\u{0000}�xx" (Unknown "\u{0000}�xx")
            , p "truncated val" "val OP_C.CRSE 0.06" (Unknown "val OP_C.CRSE 0.06")
            , p "bad number" "val A.B x y \"t\"" (Unknown "val A.B x y \"t\"")
            , p "unterminated quote" "opt A.B 1 \"oops" (Unknown "opt A.B 1 \"oops")
            , p "bad kind" "param A.B \"L\" weird 0 1 1 0" (Unknown "param A.B \"L\" weird 0 1 1 0")
            , p "bad OK" "OK soon" (Unknown "OK soon")
            , p "lone keyword" "page" (Unknown "page")
            , test "never crashes" <|
                \_ ->
                    [ "val", "val \"", "\"", "param", "ed hello", "ed hello x", "OK -1", "part x", "route", "opt", "  " ]
                        |> List.map (parse >> isUnknown)
                        |> Expect.equal (List.repeat 11 True)
            ]
        , describe "commands"
            [ test "hello" <| \_ -> Expect.equal "ed hello" (encode Hello)
            , test "describe" <| \_ -> Expect.equal "ed describe ALGO 3" (encode (Describe "ALGO" 3))
            , test "get" <| \_ -> Expect.equal "ed get OP_C.CRSE" (encode (Get (Addr "OP_C.CRSE")))
            , test "set num" <| \_ -> Expect.equal "ed set FILTER.CUTOFF 1234.5" (encode (Set (Addr "FILTER.CUTOFF") (Num 1234.5)))
            , test "set name" <| \_ -> Expect.equal "ed set FILTER.KIND LADDER" (encode (Set (Addr "FILTER.KIND") (Name "LADDER")))
            , test "since" <| \_ -> Expect.equal "ed since 7" (encode (Since 7))
            ]
        ]


isUnknown : Line -> Bool
isUnknown l =
    case l of
        Unknown _ ->
            True

        _ ->
            False
