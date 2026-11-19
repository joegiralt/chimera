module Schema exposing
    ( Page
    , Panel
    , Param
    , Schema
    , add
    , empty
    , hasPanel
    , optionName
    , pageCount
    , pages
    , param
    , paramIn
    )

{-| What `ed describe` says: the static schema, built line by line as the
answers arrive. Pages hold panels (a variant each), panels hold params.
-}

import Dict exposing (Dict)
import Protocol exposing (Addr, AnswerLine, Flags, Kind, OptIndex, Scale, addrKey)


type alias Param =
    { addr : Addr
    , label : String
    , kind : Kind
    , scale : Scale
    , min : Float
    , max : Float
    , step : Float
    , default : Float
    , flags : Flags
    , options : List ( OptIndex, String )
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


{-| Some page of it lists a panel of that name (`REVERB`, `FILTER`).
-}
hasPanel : String -> Schema -> Bool
hasPanel name (Schema s) =
    List.any (\pg -> List.any (\pn -> pn.name == name) pg.panels) s.pages


{-| The parameter as the panels now showing describe it. One address can be
described by several variants (FILTER.MODE has different names per kind), so
`param` alone is the last described, and editing needs this: a variant panel
beats a generic one.
-}
paramIn : Dict String String -> Addr -> Schema -> Maybe Param
paramIn variants a (Schema s) =
    let
        key : String
        key =
            addrKey a

        shown : Panel -> Bool
        shown pn =
            pn.variant == "-" || Dict.get pn.name variants == Just pn.variant

        mine : Panel -> Maybe ( Bool, Param )
        mine pn =
            pn.params |> List.filter (\p -> addrKey p.addr == key) |> List.head |> Maybe.map (\p -> ( pn.variant /= "-", p ))

        found : List ( Bool, Param )
        found =
            s.pages
                |> List.concatMap (\pg -> pg.panels |> List.filter shown |> List.filterMap mine)

        specific : Maybe ( Bool, Param )
        specific =
            found |> List.filter Tuple.first |> List.head
    in
    case specific of
        Just ( _, p ) ->
            Just p

        Nothing ->
            case found of
                ( _, p ) :: _ ->
                    Just p

                [] ->
                    Dict.get key s.index


optionName : Dict String String -> Addr -> OptIndex -> Schema -> Maybe String
optionName variants a i sch =
    paramIn variants a sch
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
                q : Param
                q =
                    { addr = p.addr, label = p.label, kind = p.kind, scale = p.scale, min = p.min, max = p.max, step = p.step, default = p.default, flags = p.flags, options = [] }
            in
            Schema
                { s
                    | pages = onLastPanel (\pn -> { pn | params = pn.params ++ [ q ] }) s.pages
                    , index = Dict.insert (addrKey p.addr) q s.index
                }

        Protocol.Opt o ->
            let
                withOpt : Param -> Param
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

        -- TAPE's SOURCE is read-only: its text comes with `since`.
        Protocol.Source _ ->
            Schema s

        Protocol.HelloIs _ ->
            Schema s

        Protocol.Val _ ->
            Schema s

        Protocol.PartIs _ _ ->
            Schema s

        Protocol.SchemaIs _ ->
            Schema s

        Protocol.MixerIs ->
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
