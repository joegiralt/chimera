module TokensTest exposing (suite)

import Expect
import Test exposing (Test, describe, test)
import Ui.Tokens as Tokens exposing (Os(..), Theme(..))


suite : Test
suite =
    describe "Ui.Tokens"
        [ test "a_stored_choice_wins_over_the_os" <|
            \_ ->
                Expect.equal [ Day, Night ]
                    [ Tokens.fromPreference { stored = Just "day", os = OsDark }
                    , Tokens.fromPreference { stored = Just "night", os = OsLight }
                    ]
        , test "without_a_choice_the_os_decides" <|
            \_ ->
                Expect.equal [ Night, Day ]
                    [ Tokens.fromPreference { stored = Nothing, os = OsDark }
                    , Tokens.fromPreference { stored = Nothing, os = OsLight }
                    ]
        , test "a_stored_word_that_is_no_theme_is_ignored" <|
            \_ ->
                Tokens.fromPreference { stored = Just "sepia", os = OsLight }
                    |> Expect.equal Day
        , test "a_theme_stores_as_its_name_and_reads_back" <|
            \_ ->
                List.map (Tokens.name >> Tokens.fromName) Tokens.themes
                    |> Expect.equal [ Just Day, Just Night ]
        , test "a_token_is_a_css_variable" <|
            \_ ->
                Tokens.var Tokens.Mod
                    |> Expect.equal "var(--mod)"
        ]
