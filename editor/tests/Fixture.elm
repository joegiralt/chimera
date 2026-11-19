module Fixture exposing (param, schema)

import FixtureData
import Protocol exposing (Addr(..), Line(..))
import Schema exposing (Schema)


{-| What the unit's `ed describe ALGO` answers, read the way the model reads it.
-}
schema : Schema
schema =
    List.foldl
        (\raw s ->
            case Protocol.parse raw of
                Answer a ->
                    Schema.add a s

                Ok_ _ ->
                    s

                OkMore _ ->
                    s

                Queued ->
                    s

                Err_ _ ->
                    s

                SinceTooLarge ->
                    s

                Unknown ->
                    s
        )
        Schema.empty
        FixtureData.lines


{-| A parameter the fixture must hold.
-}
param : String -> Schema.Param
param a =
    case Schema.param (Addr a) schema of
        Just p ->
            p

        Nothing ->
            Debug.todo ("the fixture has no " ++ a)
