module Layout.Algo exposing
    ( Cell
    , Control(..)
    , Entry
    , Item
    , Placed
    , Screen
    , Section(..)
    , SectionBox
    , When
    , items
    , place
    , routes
    , screens
    , sections
    , table
    , unplaced
    )

{-| The look of the ALGO panel: which section and which unit-grid cell each
parameter sits in, as a Prophet-6 style face (mockup v10). Only the look is
here. Labels, ranges and option names come from the schema; a parameter the
unit describes and this table doesn't place goes to `unplaced`, never hidden.
-}

import Dict exposing (Dict)
import Protocol exposing (Addr, addrKey)
import Schema exposing (Schema)


type Section
    = Algorithm
    | Operator Int
    | Filter
    | FilterMod
    | Jacks
    | VoiceFx
    | Amp
    | Envelope Int
    | Lfo Int
    | Pitch
    | Sends


{-| How a parameter is worked. A `Hero` is the one big knob of a section.
`Choice` is a list of lit options (a stepped knob when there are many);
`Toggle` is a lever between printed positions.
-}
type Control
    = Knob
    | Hero
    | Slider
    | Choice
    | Toggle


{-| Units, counted from 1: column, row, width, height.
-}
type alias Cell =
    { col : Int, row : Int, w : Int, h : Int }


type alias Placed =
    { section : Section, cell : Cell, control : Control }


{-| A placement holds always, or only while a panel shows a variant: the
SEM filter's MODE is its OUTPUT switch, not a mode cell.
-}
type When
    = Always
    | While String String


type alias Entry =
    { addr : String, when : When, placed : Placed }


type alias SectionBox =
    { section : Section, title : String, box : Cell }


{-| A screen's place. Slice 1 draws a plain placeholder in it.
-}
type alias Screen =
    { section : Section, cell : Cell, title : String }



-- THE FACE


{-| A section's title, its number in it: one place for both.
-}
title : Section -> String
title s =
    case s of
        Algorithm ->
            "Algorithm"

        Operator n ->
            "Operator " ++ String.fromInt n

        Filter ->
            "Filter"

        FilterMod ->
            "Filter Mod"

        Jacks ->
            "Jacks"

        VoiceFx ->
            "Voice FX"

        Amp ->
            "Amp"

        Envelope n ->
            "Envelope " ++ String.fromInt n

        Lfo n ->
            "LFO " ++ String.fromInt n

        Pitch ->
            "Pitch"

        Sends ->
            "Out / Sends"


boxed : Section -> Cell -> SectionBox
boxed s c =
    { section = s, title = title s, box = c }


sections : List SectionBox
sections =
    boxed Algorithm (Cell 1 1 2 6)
        :: List.map (\n -> boxed (Operator n) (Cell 3 n 20 1)) (List.range 1 6)
        ++ [ boxed Filter (Cell 1 7 6 2)
           , boxed FilterMod (Cell 7 7 3 2)
           , boxed Jacks (Cell 10 7 2 2)
           , boxed VoiceFx (Cell 12 7 5 2)
           , boxed Amp (Cell 17 7 4 2)
           ]
        ++ List.map (\n -> boxed (Envelope n) (Cell 1 (8 + n) 9 1)) (List.range 1 3)
        ++ List.map (\n -> boxed (Lfo n) (Cell 10 (8 + n) 7 1)) (List.range 1 3)
        ++ [ boxed Pitch (Cell 17 9 2 3)
           , boxed Sends (Cell 19 9 2 3)
           ]


screens : List Screen
screens =
    [ Screen Algorithm (Cell 1 1 2 2) "ALG A > B"
    , Screen Filter (Cell 1 1 2 2) "RESPONSE"
    , Screen VoiceFx (Cell 1 1 2 2) "FX CURVE"
    , Screen Amp (Cell 1 1 2 2) "VACTROL"
    , Screen Pitch (Cell 1 1 1 1) "TUNER"
    , Screen Sends (Cell 1 1 1 1) "FLOW"
    ]
        ++ List.concatMap (\n -> [ Screen (Operator n) (Cell 1 1 1 1) "WAVE", Screen (Operator n) (Cell 10 1 1 1) "ENVELOPE" ]) (List.range 1 6)
        ++ List.map (\n -> Screen (Envelope n) (Cell 2 1 1 1) "ENV") (List.range 1 3)
        ++ List.map (\n -> Screen (Lfo n) (Cell 2 1 1 1) "WAVE") (List.range 1 3)



-- THE TABLE


put : Section -> String -> Control -> Cell -> Entry
put s a c cell =
    { addr = a, when = Always, placed = { section = s, cell = cell, control = c } }


one : Section -> String -> Control -> Int -> Int -> Entry
one s a c col row =
    put s a c (Cell col row 1 1)


table : List Entry
table =
    algorithm
        ++ List.concatMap operator (List.range 1 6)
        ++ filter
        ++ filterMod
        ++ jacks
        ++ voiceFx
        ++ amp
        ++ List.concatMap envelope (List.range 1 3)
        ++ List.concatMap lfo (List.range 1 3)
        ++ pitch
        ++ sends


algorithm : List Entry
algorithm =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one Algorithm
    in
    [ o "ALGO.ALG_A" Choice 1 3
    , o "ALGO.ALG_B" Choice 2 3
    , put Algorithm "ALGO.MORPH" Hero (Cell 1 4 2 2)
    , o "ALGO.TRNSP" Knob 1 6
    , o "ALGO.VOWEL" Knob 2 6
    ]


{-| Operator n's row: the table cells, then five envelope sliders, then the
kind's cells (FORMANT and NOISE share the first columns).
-}
operator : Int -> List Entry
operator n =
    let
        a : String -> String
        a i =
            "OP_" ++ String.fromChar (Char.fromCode (64 + n)) ++ "." ++ i

        o : String -> Control -> Int -> Entry
        o i c col =
            one (Operator n) (a i) c col 1
    in
    [ o "WAVE" Choice 2
    , o "CRSE" Choice 3
    , o "FINE" Knob 4
    , o "DETUN" Knob 5
    , o "FDBK" Knob 6
    , o "LEVEL" Knob 7
    , o "VEL" Knob 8
    , o "RS" Knob 9
    , o "AR" Slider 11
    , o "D1R" Slider 12
    , o "D1L" Slider 13
    , o "D2R" Slider 14
    , o "RR" Slider 15
    , o "FRMT" Knob 16
    , o "BW" Knob 17
    , o "SKIRT" Knob 18
    , o "RES" Knob 18
    , o "KEY" Knob 19
    , o "VSLOT" Choice 20
    ]


filter : List Entry
filter =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one Filter
    in
    [ put Filter "FILTER.CUTOFF" Hero (Cell 3 1 2 2)
    , o "FILTER.KIND" Choice 5 1
    , o "FILTER.RESO" Knob 6 1
    , o "FILTER.MODE" Choice 5 2
    , o "FILTER.MORPH" Knob 5 2
    , o "FILTER.VOICE" Toggle 5 2
    , o "FILTER.STAB" Knob 6 2
    ]


filterMod : List Entry
filterMod =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one FilterMod

        semMode : Entry
        semMode =
            o "FILTER.MODE" Choice 3 1
    in
    [ o "FILTER.FB" Knob 1 1
    , o "FILTER.LEVEL" Knob 2 1
    , { semMode | when = While "FILTER" "SEM" }
    ]


jacks : List Entry
jacks =
    [ one Jacks "FILTER.FM3IN" Choice 1 1, one Jacks "FILTER.QMIN" Choice 1 2 ]


voiceFx : List Entry
voiceFx =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one VoiceFx
    in
    [ o "VOICEFX.TYPE" Choice 3 1
    , o "VOICEFX.PLACE" Toggle 4 1
    , o "VOICEFX.MIX" Knob 5 1
    , o "VOICEFX.P1" Knob 3 2
    , o "VOICEFX.P2" Knob 4 2
    , o "VOICEFX.P3" Knob 5 2
    ]


amp : List Entry
amp =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one Amp
    in
    [ o "OUT.LPG" Knob 3 1, o "OUT.DECAY" Knob 4 1, o "OUT.STRIKE" Knob 3 2, o "OUT.VEL" Knob 4 2 ]


{-| Envelope n: A's five sliders or B's three and its MODE and FORM switches
(the addresses differ, so one column list serves both).
-}
envelope : Int -> List Entry
envelope n =
    let
        a : String -> String
        a i =
            "ENV" ++ String.fromInt n ++ "." ++ i

        o : String -> Control -> Int -> Entry
        o i c col =
            one (Envelope n) (a i) c col 1
    in
    [ o "TYPE" Toggle 1
    , o "ATK" Slider 3
    , o "H" Slider 4
    , o "DEC" Slider 5
    , o "SUS" Slider 6
    , o "REL" Slider 7
    , o "MODE" Toggle 3
    , o "RISE" Slider 4
    , o "FALL" Slider 5
    , o "SHAPE" Slider 6
    , o "FORM" Toggle 7
    , o "SPEED" Toggle 8
    , o "HOLD" Toggle 9
    ]


lfo : Int -> List Entry
lfo n =
    let
        a : String -> String
        a i =
            "LFO" ++ String.fromInt n ++ "." ++ i

        o : String -> Control -> Int -> Entry
        o i c col =
            one (Lfo n) (a i) c col 1
    in
    [ o "TYPE" Toggle 1
    , o "RATE" Knob 3
    , o "SHAPE" Choice 4
    , o "SYNC" Toggle 5
    , o "PHASE" Knob 6
    , o "DEPTH" Knob 7
    , o "RISE" Knob 4
    , o "FALL" Knob 5
    , o "SHAPE_B" Knob 6
    , o "FORM" Toggle 7
    ]


pitch : List Entry
pitch =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one Pitch
    in
    [ o "PITCH.PITCH" Knob 2 1
    , o "PITCH.FINE" Knob 1 2
    , o "PITCH.GLIDE_TIME" Knob 2 2
    , o "PITCH.STEAL" Toggle 1 3
    , o "PITCH.SNAP" Toggle 2 3
    ]


sends : List Entry
sends =
    let
        o : String -> Control -> Int -> Int -> Entry
        o =
            one Sends
    in
    [ o "OUT.LEVEL" Knob 2 1, o "OUT.SEND1" Knob 1 2, o "OUT.SEND2" Knob 2 2, o "OUT.SEND3" Knob 1 3 ]


{-| Where `addr` sits, given each panel's variant now (`FILTER` to `SEM`). A
placement tied to a variant beats the general one.
-}
place : Dict String String -> Addr -> Maybe Placed
place variants addr =
    let
        k : String
        k =
            addrKey addr

        mine : List Entry
        mine =
            List.filter (\e -> e.addr == k) table

        special : List Entry
        special =
            List.filter
                (\e ->
                    case e.when of
                        While p v ->
                            Dict.get p variants == Just v

                        Always ->
                            False
                )
                mine
    in
    case special of
        e :: _ ->
            Just e.placed

        [] ->
            mine |> List.filter (\e -> e.when == Always) |> List.head |> Maybe.map .placed



-- WHAT THE UNIT DESCRIBES, NOW


{-| A described param on its page and panel, as `items` weighs it: a
generic panel's description gives way to a variant panel's.
-}
type alias Candidate =
    { generic : Bool, page : Schema.Page, panel : Schema.Panel, param : Schema.Param }


{-| A parameter the Sound has now, as the cell shows it.
-}
type alias Item =
    { param : Schema.Param
    , label : String
    , placed : Maybe Placed
    }


active : Dict String String -> Schema.Panel -> Bool
active variants pn =
    pn.variant == "-" || Dict.get pn.name variants == Just pn.variant


{-| Every parameter of the panels the Sound shows, once each. One address can
sit on several pages (an operator's WAVE is on the OSC page and on the
operator's own): the variant panel's description wins, being the specific one.
-}
items : Schema -> Dict String String -> List Item
items sch variants =
    let
        candidates : List Candidate
        candidates =
            Schema.pages sch
                |> List.concatMap
                    (\pg ->
                        pg.panels
                            |> List.filter (active variants)
                            |> List.concatMap (\pn -> List.map (\p -> { generic = pn.variant == "-", page = pg, panel = pn, param = p }) pn.params)
                    )

        keep : Candidate -> ( List String, Dict String ( Bool, String, Schema.Param ) ) -> ( List String, Dict String ( Bool, String, Schema.Param ) )
        keep c ( order, best ) =
            let
                k : String
                k =
                    addrKey c.param.addr

                cand : ( Bool, String, Schema.Param )
                cand =
                    ( c.generic, labelOf c.page c.panel c.param, c.param )
            in
            case Dict.get k best of
                Nothing ->
                    ( order ++ [ k ], Dict.insert k cand best )

                Just ( heldGeneric, _, _ ) ->
                    if heldGeneric && not c.generic then
                        ( order, Dict.insert k cand best )

                    else
                        ( order, best )

        ( keys, chosen ) =
            List.foldl keep ( [], Dict.empty ) candidates
    in
    keys
        |> List.filterMap (\k -> Dict.get k chosen)
        |> List.map (\( _, label, p ) -> { param = p, label = label, placed = place variants p.addr })


{-| A page of all six operators labels its cells OP1..OP6; the cell then says
what the page is about.
-}
labelOf : Schema.Page -> Schema.Panel -> Schema.Param -> String
labelOf pg pn p =
    if pn.variant == "-" && isOpLabel p.label then
        pg.label

    else
        p.label


isOpLabel : String -> Bool
isOpLabel s =
    case String.toList s of
        [ 'O', 'P', d ] ->
            d >= '1' && d <= '6'

        _ ->
            False


unplaced : List Item -> List Item
unplaced =
    List.filter (\i -> i.placed == Nothing)


{-| The matrix routes the shown panels name, for the slice that wires them.
-}
routes : Schema -> Dict String String -> List String
routes sch variants =
    Schema.pages sch
        |> List.concatMap (\pg -> pg.panels |> List.filter (active variants) |> List.concatMap .routes)
        |> List.foldl
            (\r acc ->
                if List.member r acc then
                    acc

                else
                    acc ++ [ r ]
            )
            []
