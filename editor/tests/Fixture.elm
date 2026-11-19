module Fixture exposing (param, paramIn, schema, schemaFor)

import FixtureData
import Protocol exposing (Line(..))
import Schema exposing (Schema)
import Unit exposing (Engine(..))
import Wire exposing (addr)


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

                SetRefused _ _ ->
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
param =
    paramIn schema


{-| What a test gets for an address its fixture lacks: a param no
expectation can mistake for a real one (its label says so, its range is
empty), so the test fails on it.
-}
missing : String -> Schema.Param
missing a =
    { addr = addr a
    , label = "NOT IN THE FIXTURE: " ++ a
    , kind = Protocol.Cont
    , scale = Protocol.Linear
    , min = 0
    , max = 0
    , step = 0
    , default = 0
    , options = []
    }


{-| A parameter another fixture must hold.
-}
paramIn : Schema -> String -> Schema.Param
paramIn sch a =
    case Schema.param (addr a) sch of
        Just p ->
            p

        Nothing ->
            missing a
