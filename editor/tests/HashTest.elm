module HashTest exposing (suite)

import Expect
import FixtureData
import Hash
import Test exposing (Test, describe, test)


{-| `describe`'s own lines of a fixture: its text without each page's
closing `OK`, which the unit's hash doesn't read (`ed::hash` walks
`describe_all`; `OK` is the command's).
-}
described : List String -> List String
described =
    List.filter (\l -> l /= "OK")


hex : List String -> String
hex ls =
    Hash.toHex (Hash.ofLines ls)


fixture : String -> List String
fixture name =
    [ ( "algo", FixtureData.lines )
    , ( "va", FixtureData.va )
    , ( "modal", FixtureData.modal )
    , ( "tape", FixtureData.tape )
    , ( "drum", FixtureData.drum )
    , ( "mixer", FixtureData.mixer )
    , ( "settings", FixtureData.settings )
    , ( "map", FixtureData.map )
    , ( "orbit", FixtureData.orbit )
    ]
        |> List.filter (\( n, _ ) -> n == name)
        |> List.concatMap Tuple.second


suite : Test
suite =
    describe "Hash"
        [ test "nothing_hashes_to_fnv_1a_64s_offset" <|
            \_ -> Expect.equal "cbf29ce484222325" (hex [])
        , -- FNV-1a 64's published vectors, each with the line's `\n`
          -- ("a\n", "foobar\n"), as node's BigInt folds them.
          test "a_line_is_its_bytes_and_a_newline" <|
            \_ -> Expect.equal [ "089bdc07b544e7b2", "345316a7168b8506" ] [ hex [ "a" ], hex [ "foobar" ] ]
        , test "lines_hash_as_one_text" <|
            \_ -> Expect.equal (hex [ "a\nb" ]) (hex [ "a", "b" ])
        , test "a_character_past_ascii_hashes_as_its_utf_8_bytes" <|
            \_ -> Expect.equal [ "68a23c1c22ee9134", "ee9e17c8a89506d3", "064c76faaa4e9f76" ] [ hex [ "·" ], hex [ "€" ], hex [ "𝄞" ] ]
        , test "one_byte_moves_it" <|
            \_ -> Expect.notEqual (hex [ "page 0 ALG of 1" ]) (hex [ "page 0 ALH of 1" ])
        , test "sixteen_hex_digits_read_and_write_the_same" <|
            \_ ->
                [ "0123456789abcdef", "0000000000000000", "ffffffffffffffff" ]
                    |> List.map (Hash.fromHex >> Maybe.map Hash.toHex)
                    |> Expect.equal [ Just "0123456789abcdef", Just "0000000000000000", Just "ffffffffffffffff" ]
        , test "anything_else_is_no_hash" <|
            \_ ->
                [ "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "0123456789abcdeg", "" ]
                    |> List.map Hash.fromHex
                    |> Expect.equal (List.repeat 5 Nothing)
        , test "a_hash_read_equals_the_one_computed" <|
            \_ -> Expect.equal (Hash.fromHex "cbf29ce484222325") (Just (Hash.ofLines []))
        , -- The unit's `ed schema` hash of each fixture: node's own FNV-1a 64
          -- of the file's bytes without its `OK` lines (gen-fixture.js).
          test "every_fixture_hashes_as_the_unit_hashes_describes_bytes" <|
            \_ ->
                FixtureData.schemaHashes
                    |> List.map (\( name, _ ) -> ( name, hex (described (fixture name)) ))
                    |> Expect.equal FixtureData.schemaHashes
        , test "every_fixture_is_hashed" <|
            \_ ->
                FixtureData.schemaHashes
                    |> List.map Tuple.first
                    |> Expect.equal [ "algo", "va", "modal", "tape", "drum", "mixer", "settings", "map", "orbit" ]
        ]
