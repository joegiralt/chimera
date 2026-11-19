module Ui.Routes exposing
    ( Amount
    , Cable
    , Dest(..)
    , Routes
    , Sign(..)
    , Source(..)
    , add
    , amount
    , amountFromNorm
    , amountNorm
    , amountText
    , amountValue
    , cables
    , dests
    , new
    , remove
    , setAmount
    , sign
    , sources
    )

{-| The routes of a mod matrix: sources, destinations, and the cables
between them, each with a signed amount. The patch bay and the matrix grid
are two views of this one model.
-}

import Protocol exposing (Addr)


type Source
    = Source String


type Dest
    = Dest String


{-| A cable's amount, −100 to +100.
-}
type Amount
    = Amount Int


amount : Int -> Amount
amount =
    clamp -100 100 >> Amount


amountValue : Amount -> Int
amountValue (Amount a) =
    a


amountText : Amount -> String
amountText (Amount a) =
    if a > 0 then
        "+" ++ String.fromInt a

    else
        String.fromInt a


{-| Where the attenuverter points: −100 at 0, nothing at the middle.
-}
amountNorm : Amount -> Float
amountNorm (Amount a) =
    (toFloat a + 100) / 200


amountFromNorm : Float -> Amount
amountFromNorm t =
    amount (round ((clamp 0 1 t - 0.5) * 200))


{-| A cable's colour: amber adds, blue subtracts.
-}
type Sign
    = Adds
    | Subtracts


sign : Amount -> Sign
sign (Amount a) =
    if a < 0 then
        Subtracts

    else
        Adds


{-| A cable, and the address its amount is kept at.
-}
type alias Cable =
    { from : Source, to : Dest, amount : Amount, addr : Addr }


type Routes
    = Routes { sources : List Source, dests : List Dest, cables : List Cable }


new : List Source -> List Dest -> Routes
new ss ds =
    Routes { sources = ss, dests = ds, cables = [] }


sources : Routes -> List Source
sources (Routes r) =
    r.sources


dests : Routes -> List Dest
dests (Routes r) =
    r.dests


cables : Routes -> List Cable
cables (Routes r) =
    r.cables


{-| One cable between two jacks: a second replaces the first.
-}
add : Cable -> Routes -> Routes
add c (Routes r) =
    Routes { r | cables = List.filter (\x -> x.from /= c.from || x.to /= c.to) r.cables ++ [ c ] }


remove : Source -> Dest -> Routes -> Routes
remove s d (Routes r) =
    Routes { r | cables = List.filter (\x -> x.from /= s || x.to /= d) r.cables }


setAmount : Addr -> Amount -> Routes -> Routes
setAmount a v (Routes r) =
    Routes
        { r
            | cables =
                List.map
                    (\c ->
                        if c.addr == a then
                            { c | amount = v }

                        else
                            c
                    )
                    r.cables
        }
