module Fixture exposing (param, paramIn, schema, schemaFor)

import FixtureData
import Protocol exposing (Addr(..), Line(..))
import Schema exposing (Schema)
import Unit exposing (Engine(..))


{-| What the unit's `ed describe ALGO` answers, read the way the model reads it.
-}
schema : Schema
schema =
    read FixtureData.lines


{-| Each engine's `ed describe` answer.
-}
schemaFor : Engine -> Schema
schemaFor e =
    case e of
        Algo ->
            schema

        Va ->
            read FixtureData.va

        Modal ->
            read FixtureData.modal

        Tape ->
            read FixtureData.tape

        Drum ->
            read FixtureData.drum


read : List String -> Schema
read =
    List.foldl
        (\raw s ->
            case Protocol.parse raw of
                Answer a ->
                    Schema.add a s

                Ok_ _ ->
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


{-| A parameter the fixture must hold.
-}
param : String -> Schema.Param
param a =
    case Schema.param (Addr a) schema of
        Just p ->
            p

        Nothing ->
            Debug.todo ("the fixture has no " ++ a)


{-| A parameter another fixture must hold.
-}
paramIn : Schema -> String -> Schema.Param
paramIn sch a =
    case Schema.param (Addr a) sch of
        Just p ->
            p

        Nothing ->
            Debug.todo ("the fixture has no " ++ a)
