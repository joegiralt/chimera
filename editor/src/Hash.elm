module Hash exposing (Hash, fromHex, ofLines, toHex)

{-| A schema's hash as the unit's `ed schema` says it (ADR 0289): the 64-bit
FNV-1a of every byte `ed describe` writes for it, in page order. Each line's
bytes and its `\n` are in it; a page's closing `OK` is the command's, not
describe's, and is not.

Elm's integers are 32 bits under `Bitwise`, so the 64 are four 16-bit limbs.

-}

import Bitwise


{-| Sixty-four bits, most significant limb first. Opaque: read off the wire
(`fromHex`) or computed (`ofLines`), so two that are equal are the same text.
-}
type Hash
    = Hash Int Int Int Int


{-| FNV-1a 64's offset basis, `cbf29ce484222325`.
-}
offset : Hash
offset =
    Hash 0xCBF2 0x9CE4 0x8422 0x2325


{-| The hash of these lines as the unit wrote them, each ended by `\n`.
-}
ofLines : List String -> Hash
ofLines =
    List.foldl (\l h -> byte 10 (String.foldl char h l)) offset


{-| One character in: its UTF-8 bytes, as the unit wrote them.
-}
char : Char -> Hash -> Hash
char c h =
    let
        n : Int
        n =
            Char.toCode c

        -- A continuation byte: six bits of `n` from bit `shift` up.
        tail : Int -> Hash -> Hash
        tail shift =
            byte (Bitwise.or 0x80 (Bitwise.and 0x3F (Bitwise.shiftRightZfBy shift n)))
    in
    if n < 0x80 then
        byte n h

    else if n < 0x0800 then
        h |> byte (Bitwise.or 0xC0 (Bitwise.shiftRightZfBy 6 n)) |> tail 0

    else if n < 0x00010000 then
        h |> byte (Bitwise.or 0xE0 (Bitwise.shiftRightZfBy 12 n)) |> tail 6 |> tail 0

    else
        h |> byte (Bitwise.or 0xF0 (Bitwise.shiftRightZfBy 18 n)) |> tail 12 |> tail 6 |> tail 0


{-| One byte in: xor, then times FNV's prime `0x100000001b3` (2^40 + 435),
mod 2^64. Times 435 limb by limb with its carries; times 2^40 is two limbs and
eight bits up, so only the low three bytes reach the top two limbs.
-}
byte : Int -> Hash -> Hash
byte b (Hash a3 a2 a1 a0) =
    let
        x0 : Int
        x0 =
            Bitwise.xor a0 b

        p0 : Int
        p0 =
            x0 * 435

        p1 : Int
        p1 =
            a1 * 435 + Bitwise.shiftRightZfBy 16 p0

        p2 : Int
        p2 =
            a2 * 435 + Bitwise.shiftRightZfBy 16 p1 + low (Bitwise.shiftLeftBy 8 x0)

        p3 : Int
        p3 =
            a3 * 435 + Bitwise.shiftRightZfBy 16 p2 + Bitwise.shiftRightZfBy 8 x0 + low (Bitwise.shiftLeftBy 8 a1)
    in
    Hash (low p3) (low p2) (low p1) (low p0)


low : Int -> Int
low =
    Bitwise.and 0xFFFF


{-| Sixteen lowercase hex digits, as `ed schema` writes them.
-}
toHex : Hash -> String
toHex (Hash a3 a2 a1 a0) =
    String.concat (List.map limbHex [ a3, a2, a1, a0 ])


limbHex : Int -> String
limbHex n =
    let
        hexDigit : Int -> String
        hexDigit shift =
            let
                d : Int
                d =
                    Bitwise.and 0x0F (Bitwise.shiftRightZfBy shift n)
            in
            String.slice d (d + 1) "0123456789abcdef"
    in
    String.concat (List.map hexDigit [ 12, 8, 4, 0 ])


{-| Sixteen lowercase hex digits and nothing else, else `Nothing`.
-}
fromHex : String -> Maybe Hash
fromHex s =
    if String.length s == 16 then
        Maybe.map4 Hash
            (limb (String.slice 0 4 s))
            (limb (String.slice 4 8 s))
            (limb (String.slice 8 12 s))
            (limb (String.slice 12 16 s))

    else
        Nothing


limb : String -> Maybe Int
limb =
    String.foldl (\c acc -> Maybe.map2 (\n d -> n * 16 + d) acc (digit c)) (Just 0)


digit : Char -> Maybe Int
digit c =
    if Char.isDigit c then
        Just (Char.toCode c - 48)

    else if c >= 'a' && c <= 'f' then
        Just (Char.toCode c - 87)

    else
        Nothing
