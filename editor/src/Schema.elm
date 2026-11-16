module Schema exposing
    ( Page
    , Panel
    , Param
    , Schema
    , add
    , empty
    , optionName
    , pageCount
    , pages
    , param
    )

{-| What `ed describe` says: the static schema, built line by line as the
answers arrive. Pages hold panels (a variant each), panels hold params.
-}

import Dict exposing (Dict)
import Protocol exposing (Addr, AnswerLine, Kind, addrKey)


type alias Param =
    { addr : Addr
    , label : String
    , kind : Kind
    , min : Float
    , max : Float
    , step : Float
    , default : Float
    , options : List ( Int, String )
    }


type alias Panel =
    { name : String
    , variant : String
    , params : List Param
    , routes : List String
    }


type alias Page =
    { n : Int
    , label : String
    , panels : List Panel
    }


type Schema
    = Schema
        { count : Maybe Int
        , pages : List Page
        , index : Dict String Param
        }


empty : Schema
empty =
    Schema { count = Nothing, pages = [], index = Dict.empty }


{-| How many pages the unit says there are, once one has been described.
-}
pageCount : Schema -> Maybe Int
pageCount (Schema s) =
    s.count


pages : Schema -> List Page
pages (Schema s) =
    s.pages


param : Addr -> Schema -> Maybe Param
param a (Schema s) =
    Dict.get (addrKey a) s.index


optionName : Addr -> Int -> Schema -> Maybe String
optionName a i sch =
    param a sch
        |> Maybe.andThen (\p -> p.options |> List.filter (\( k, _ ) -> k == i) |> List.head)
        |> Maybe.map Tuple.second


{-| Take one describe line in. A page described again replaces its old self;
a panel, param or option attaches to the latest of the thing above it.
-}
add : AnswerLine -> Schema -> Schema
add line (Schema s) =
    case line of
        Protocol.Page p ->
            Schema
                { s
                    | count = Just p.count
                    , pages = List.filter (\x -> x.n /= p.n) s.pages ++ [ { n = p.n, label = p.label, panels = [] } ]
                }

        Protocol.Panel p ->
            Schema { s | pages = onLast (\pg -> { pg | panels = pg.panels ++ [ { name = p.name, variant = p.variant, params = [], routes = [] } ] }) s.pages }

        Protocol.Param p ->
            let
                q =
                    { addr = p.addr, label = p.label, kind = p.kind, min = p.min, max = p.max, step = p.step, default = p.default, options = [] }
            in
            Schema
                { s
                    | pages = onLastPanel (\pn -> { pn | params = pn.params ++ [ q ] }) s.pages
                    , index = Dict.insert (addrKey p.addr) q s.index
                }

        Protocol.Opt o ->
            let
                withOpt q =
                    if q.addr == o.addr then
                        { q | options = q.options ++ [ ( o.index, o.name ) ] }

                    else
                        q
            in
            Schema
                { s
                    | pages = onLastPanel (\pn -> { pn | params = List.map withOpt pn.params }) s.pages
                    , index = Dict.update (addrKey o.addr) (Maybe.map withOpt) s.index
                }

        Protocol.Route r ->
            Schema { s | pages = onLastPanel (\pn -> { pn | routes = pn.routes ++ [ r ] }) s.pages }

        _ ->
            Schema s


onLastPanel : (Panel -> Panel) -> List Page -> List Page
onLastPanel f =
    onLast (\pg -> { pg | panels = onLast f pg.panels })


onLast : (a -> a) -> List a -> List a
onLast f xs =
    case List.reverse xs of
        last :: before ->
            List.reverse (f last :: before)

        [] ->
            []
