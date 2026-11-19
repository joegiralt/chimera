module ReviewConfig exposing (config)

{-| The editor's gate: the site's own rule set (chimera-site site/review),
every rule over src and tests alike. NoUnused, NoDebug,
NoMissingTypeAnnotation and the common set, Simplify, and no catch-all over
a custom type.
-}

import NoCatchAllForSpecificRemainingPatterns
import NoDebug.Log
import NoDebug.TodoOrToString
import NoExposingEverything
import NoImportingEverything
import NoMissingTypeAnnotation
import NoMissingTypeAnnotationInLetIn
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
      -- every case lists them.
      NoCatchAllForSpecificRemainingPatterns.rule { onlyReportCatchAllIfEquivalentToSinglePattern = False }
    , NoDebug.Log.rule
    , NoDebug.TodoOrToString.rule
    , NoExposingEverything.rule
    , NoImportingEverything.rule []
    , NoMissingTypeAnnotation.rule
    , NoMissingTypeAnnotationInLetIn.rule
    , NoMissingTypeExpose.rule
        -- An application's Main (and the gallery's) exposes only `main`.
        |> Review.Rule.ignoreErrorsForFiles [ "src/Main.elm", "src/Gallery.elm" ]
    , NoPrematureLetComputation.rule
    , NoUnused.CustomTypeConstructorArgs.rule
    , NoUnused.CustomTypeConstructors.rule []
    , NoUnused.Dependencies.rule
    , NoUnused.Exports.rule
    , NoUnused.Parameters.rule
    , NoUnused.Patterns.rule
    , NoUnused.Variables.rule
    , Simplify.rule Simplify.defaults
    ]
