module UnitTest exposing (suite)

import Expect
import FixtureData
import Test exposing (Test, describe, test)
import Unit


suite : Test
suite =
    describe "Unit"
        [ test "a_part_is_one_of_the_boxs_six" <|
            \_ ->
                [ 0, 1, 6, 7, -1 ]
                    |> List.map (Unit.partFromInt >> Maybe.map Unit.partNumber)
                    |> Expect.equal [ Nothing, Just 1, Just 6, Nothing, Nothing ]
        , test "all_parts_in_order" <|
            \_ -> Expect.equal [ 1, 2, 3, 4, 5, 6 ] (List.map Unit.partNumber Unit.allParts)
            , test "the_unit_has_as_many_parts_as_the_editor" <|
            \_ ->
                -- The unit's own refusal (fixtures/part-refusal.txt, from console_ed_test).
                Expect.equal ("ERR ed part takes 1-" ++ String.fromInt Unit.partCount) FixtureData.partRefusal
        , test "part_count_is_the_list" <|
            \_ -> Expect.equal (List.length Unit.allParts) Unit.partCount
        ]
