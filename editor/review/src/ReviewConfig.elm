module ReviewConfig exposing (config)

{-| The editor's best-practice gate, the site's own (chimera-site
site/review): NoUnused, NoDebug, NoMissingTypeAnnotation and the common
set, Simplify, and no catch-all over a custom type.
-}

import NoCatchAllForSpecificRemainingPatterns
import NoDebug.Log
import NoDebug.TodoOrToString
import NoExposingEverything
import NoImportingEverything
import NoMissingTypeAnnotation
import NoMissingTypeExpose
import NoPrematureLetComputation
import NoUnused.CustomTypeConstructorArgs
import NoUnused.CustomTypeConstructors
import NoUnused.Dependencies
import NoUnused.Exports
import NoUnused.Parameters
import NoUnused.Patterns
import NoUnused.Variables
import Review.Rule exposing (Rule)
import Simplify


config : List Rule
config =
    [ -- A `_` over a custom type hides a new constructor (a new Route):
      -- every case lists them (a new Engine, Line or Stage).
      NoCatchAllForSpecificRemainingPatterns.rule { onlyReportCatchAllIfEquivalentToSinglePattern = False }
    , NoDebug.Log.rule
    , NoDebug.TodoOrToString.rule
        -- A fixture that lacks an address fails its test loudly.
        |> Review.Rule.ignoreErrorsForDirectories [ "tests/" ]
    , NoExposingEverything.rule
    , NoImportingEverything.rule []
    , NoMissingTypeAnnotation.rule

    -- NoMissingTypeAnnotationInLetIn (the site's) is not taken yet: slice
    -- 1's lets carry about ninety unannotated bindings.
    , NoMissingTypeExpose.rule
        -- An application's Main exposes only `main`.
        |> Review.Rule.ignoreErrorsForFiles [ "src/Main.elm" ]
    , NoPrematureLetComputation.rule
    , NoUnused.CustomTypeConstructorArgs.rule
        -- Told apart by (==), never taken apart: a Section's number, a
        -- control's kind on a wheel, an unknown line's text, a test's Msg.
        |> Review.Rule.ignoreErrorsForFiles [ "src/Layout/Algo.elm", "src/View/Controls.elm", "src/Protocol.elm" ]
        |> Review.Rule.ignoreErrorsForDirectories [ "tests/" ]
    , NoUnused.CustomTypeConstructors.rule []
    , NoUnused.Dependencies.rule
    , NoUnused.Exports.rule
    , NoUnused.Parameters.rule
    , NoUnused.Patterns.rule
    , NoUnused.Variables.rule

    -- The protocol's tests encode NaN on purpose (it is refused).
    , Simplify.rule (Simplify.defaults |> Simplify.expectNaN)
    ]
