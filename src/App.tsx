import { lazy, Suspense, type Component } from "solid-js";
import { MemoryRouter, createMemoryHistory, Route } from "@solidjs/router";
import Sidebar from "./components/Sidebar";
import CardOverlay from "./components/CardOverlay";
import DeckOverlay from "./components/DeckOverlay";
import UpdateBanner from "./components/UpdateBanner";

const Collection = lazy(() => import("./views/Collection"));
const Economy = lazy(() => import("./views/Economy"));
const Events = lazy(() => import("./views/Events"));
const Feed = lazy(() => import("./views/Feed"));
const Decks = lazy(() => import("./views/Decks"));
const Settings = lazy(() => import("./views/Settings"));

const App: Component = () => {
  const history = createMemoryHistory();

  return (
    <div class="app-shell">
      <UpdateBanner />
      <div class="app">
        <Sidebar history={history} />
        <main class="content">
          <MemoryRouter
            history={history}
            root={(props) => <Suspense>{props.children}</Suspense>}
          >
            <Route path="/" component={Collection} />
            <Route path="/economy" component={Economy} />
            <Route path="/events" component={Events} />
            <Route path="/feed" component={Feed} />
            <Route path="/decks" component={Decks} />
            <Route path="/settings" component={Settings} />
          </MemoryRouter>
        </main>
        <DeckOverlay />
        <CardOverlay />
      </div>
    </div>
  );
};

export default App;
