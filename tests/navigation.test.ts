import assert from "node:assert/strict";
import { test } from "node:test";
import {
  createRouter,
  type HistoryState,
  type Router,
  type RouterHistory,
  type NavigationCallback,
} from "vue-router";
import { createLayerNavigation } from "../src/utils/navigation/controller";
import { HISTORY_KEY, pageCacheKey, remember } from "../src/utils/navigation/layers";
import { readLayerHistory } from "../src/utils/navigation/history";
import { imagePixelRect } from "../src/utils/navigation/sources";
import {
  projectedCoverFrames,
  projectedSurfaceClips,
  sharedImageFrames,
  type SurfaceScale,
} from "../src/utils/navigation/surfaceProjection";
import { decorationRect, type SourceDecoration } from "../src/utils/navigation/sourceAppearance";

test("shared image corners follow the image's own scale, including hover and interrupted states", () => {
  const avatar = { rect: { x: 25.75, y: 387.1, width: 69.7, height: 69.7 }, radius: 34.85 };
  const detail = { rect: { x: 77, y: 72, width: 235, height: 235 }, radius: 8 };
  const interrupted = { rect: { x: 54, y: 210, width: 160, height: 160 }, radius: 42 };
  for (const [from, to] of [
    [avatar, detail],
    [detail, avatar],
    [interrupted, avatar],
  ]) {
    const frames = sharedImageFrames(from, to, { x: 20, y: 60, width: 300, height: 400 });
    for (const [index, shape] of [from, to].entries()) {
      const scale = Number(frames.transform[index].split("scale(")[1].split(",")[0]);
      const fraction = Number.parseFloat(frames.borderRadius[index]) / 100;
      assert.ok(frames.borderRadius[index].endsWith("%"));
      assert.ok(Math.abs(to.rect.width * scale - shape.rect.width) < 1e-10);
      assert.ok(Math.abs(to.rect.width * scale * fraction - shape.radius) < 1e-10);
    }
  }
});

test("cover roundness and image size share the same progress in both directions and after interruption", () => {
  const avatar = { rect: { x: 26, y: 388, width: 68, height: 68 }, radius: 34 };
  const detail = { rect: { x: 77, y: 72, width: 235, height: 235 }, radius: 8 };
  const interrupted = projectedCoverFrames(avatar, detail)[50];
  for (const [from, to] of [
    [avatar, detail],
    [detail, avatar],
    [interrupted, avatar],
  ]) {
    const start = from.radius / from.rect.width;
    const end = to.radius / to.rect.width;
    const frames = projectedCoverFrames(from, to);
    assert.deepEqual(frames[0], from);
    for (const { rect, radius } of frames) {
      const sizeProgress = (rect.width - from.rect.width) / (to.rect.width - from.rect.width);
      const shapeProgress = (radius / rect.width - start) / (end - start);
      assert.ok(Math.abs(sizeProgress - shapeProgress) < 1e-10);
      assert.equal(rect.width, rect.height);
      assert.ok(radius <= rect.width / 2 && radius >= 0);
    }
  }
});

test("compact-card corners stay circular and bounded throughout a non-uniform projection", () => {
  const viewport = { x: 0, y: 0, width: 390, height: 844 };
  const card = { x: 359 / 390, y: 90 / 844, radiusX: 12, radiusY: 12 };
  const expanded = { x: 1, y: 1, radiusX: 0, radiusY: 0 };
  const interrupted = { x: 0.953, y: 0.48, radiusX: 7.1, radiusY: 7.1 };
  for (const [from, to] of [
    [card, expanded],
    [expanded, card],
    [interrupted, card],
  ] as [SurfaceScale, SurfaceScale][]) {
    const clips = projectedSurfaceClips(viewport, viewport, from, to);
    for (let step = 0; step <= 500; step++) {
      const p = step / 500;
      const cursor = p * (clips.length - 1);
      const low = Math.floor(cursor);
      const high = Math.min(low + 1, clips.length - 1);
      const sample = (index: number) =>
        clips[index].split("round ")[1].split("/").map(Number.parseFloat);
      const a = sample(low);
      const b = sample(high);
      const screen = a.map(
        (value, axis) =>
          (value + (b[axis] - value) * (cursor - low)) *
          (axis ? from.y + (to.y - from.y) * p : from.x + (to.x - from.x) * p),
      );
      const expected = from.radiusX + (to.radiusX - from.radiusX) * p;
      assert.ok(
        screen.every((radius) => Math.abs(radius - expected) < 0.03),
        `${p}: ${screen}`,
      );
      assert.ok(Math.abs(screen[0] - screen[1]) < 0.03);
    }
  }
});

test("card captions follow the cover's edge without scaling their text layout", () => {
  const source = {
    surface: { x: 10, y: 400, width: 360, height: 270 },
    cover: { x: 10, y: 400, width: 360, height: 270 },
  };
  const target = {
    surface: { x: 0, y: 0, width: 390, height: 844 },
    cover: { x: 77, y: 72, width: 235, height: 235 },
  };
  const caption: SourceDecoration = {
    key: "count",
    placement: "cover-bottom-left",
    rect: { x: 20, y: 642, width: 340, height: 18 },
    text: "1.0万",
    opacity: 1,
    style: {},
  };
  assert.deepEqual(decorationRect(caption, source, target), {
    x: 87,
    y: 279,
    width: 340,
    height: 18,
  });
  assert.deepEqual(
    decorationRect(
      {
        ...caption,
        placement: "cover-bottom",
        text: undefined,
        rect: { x: 10, y: 560, width: 360, height: 110 },
      },
      source,
      target,
    ),
    { x: 77, y: 197, width: 235, height: 110 },
  );
});

test("artwork keeps its painted pixels when a cropped or hovered card becomes a square cover", () => {
  assert.deepEqual(
    imagePixelRect(
      { x: 10, y: 100, width: 360, height: 270 },
      { width: 640, height: 640 },
      "cover",
      "50% 50%",
    ),
    { x: 10, y: 55, width: 360, height: 360 },
  );
  assert.deepEqual(
    imagePixelRect(
      { x: 5, y: 95, width: 370, height: 280 },
      { width: 640, height: 640 },
      "cover",
      "50% 50%",
    ),
    { x: 5, y: 50, width: 370, height: 370 },
  );
  assert.deepEqual(
    imagePixelRect(
      { x: 78, y: 72, width: 235, height: 235 },
      { width: 400, height: 800 },
      "cover",
      "50% 0%",
    ),
    { x: 78, y: 72, width: 235, height: 470 },
  );
});

function memoryHistory(initial = "/", data: HistoryState = {}) {
  const entries = [{ path: initial, data }];
  let cursor = 0;
  const listeners = new Set<NavigationCallback>();
  const history: RouterHistory = {
    base: "",
    get location() {
      return entries[cursor].path;
    },
    get state() {
      return entries[cursor].data;
    },
    createHref: (to) => to,
    push(to, state = {}) {
      entries.splice(++cursor);
      entries.push({ path: to, data: state });
    },
    replace(to, state = {}) {
      entries[cursor] = { path: to, data: { ...entries[cursor].data, ...state } };
    },
    go(delta, trigger = true) {
      const previous = cursor;
      cursor = Math.min(entries.length - 1, Math.max(0, cursor + delta));
      if (cursor !== previous && trigger)
        for (const listener of listeners)
          listener(entries[cursor].path, entries[previous].path, {
            delta: cursor - previous,
            direction: delta < 0 ? ("back" as any) : ("forward" as any),
            type: "pop" as any,
          });
    },
    listen(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    destroy() {
      listeners.clear();
    },
  };
  return { history, entries };
}

async function fixture(initial = "/", data: HistoryState = {}) {
  const { history, entries } = memoryHistory(initial, data);
  const page = { render: () => null };
  const router = createRouter({
    history,
    routes: [
      { path: "/", name: "home", component: page, meta: { navigationRoot: "home" } },
      { path: "/discover/:category?", component: page, meta: { navigationRoot: "discover" } },
      { path: "/local", component: page, meta: { navigationRoot: "library" } },
      { path: "/local/playlist", component: page, meta: { navigationFallback: "library" } },
      { path: "/local/song", component: page },
      {
        path: "/search",
        name: "search",
        component: page,
        children: [
          { path: "songs", name: "s-songs", component: page },
          { path: "albums", name: "s-albums", component: page },
        ],
      },
      { path: "/album", component: page },
      { path: "/artist", component: page },
      { path: "/login", component: page },
    ],
  });
  const navigation = createLayerNavigation(router);
  await router.push(initial);
  return { router, navigation, history, entries };
}

const traversed = (router: Router, action: () => void) =>
  new Promise<void>((resolve) => {
    const stop = router.afterEach(() => {
      stop();
      resolve();
    });
    action();
  });

test("page ancestry follows the actual entry and back direction", async () => {
  const { navigation: nav, router } = await fixture();
  const home = nav.current.value!.id;
  await nav.openPage("/album?id=1");
  const album = nav.current.value!.id;
  assert.equal(nav.current.value!.parentId, home);
  assert.equal(nav.current.value!.root, "home");
  await nav.openPage("/artist?id=8");
  assert.equal(nav.current.value!.parentId, album);
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.current.value!.id, album);
  assert.equal(nav.transition.value.direction, "pop");
});

test("player and its queue each consume exactly one same-URL history entry", async () => {
  const { navigation: nav, router, entries } = await fixture();
  await nav.openPlayer();
  await nav.openQueue();
  assert.equal(router.currentRoute.value.fullPath, "/");
  assert.equal(entries.length, 3);
  assert.equal(nav.current.value!.presentation, "queue-player");
  await traversed(router, () => {
    nav.closeTop();
    nav.closeTop();
  });
  assert.equal(nav.current.value!.kind, "player");
  assert.equal(nav.playerVisible.value, true);
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.current.value!.isRoot, true);
  assert.equal(nav.canGoBack.value, false);
});

test("a queue opened from a page returns directly to that page", async () => {
  const { navigation: nav, router } = await fixture();
  await nav.openQueue();
  assert.equal(nav.current.value!.presentation, "queue-floating");
  await traversed(router, () => {
    nav.closeQueue();
  });
  assert.equal(nav.state.value.layers.length, 1);
  assert.equal(nav.hasLayer("player"), false);
});

test("search input, results and categories replace the same session", async () => {
  const { navigation: nav, router, entries } = await fixture();
  await nav.openSearch();
  const session = nav.current.value!.id;
  nav.updateSearchQuery("Miles");
  await nav.replacePage("/search/songs?keywords=Miles");
  await nav.replacePage("/search/albums?keywords=Miles&page=2");
  assert.equal(nav.current.value!.id, session);
  assert.equal(entries.length, 2);
  await nav.openPage("/album?id=2");
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.current.value!.search?.query, "Miles");
  assert.equal(nav.current.value!.route, "/search/albums?keywords=Miles&page=2");
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.current.value!.isRoot, true);
});

test("a suggestion retains the underlying input session", async () => {
  const { navigation: nav, router } = await fixture();
  await nav.openSearch();
  nav.updateSearchQuery("test");
  await nav.openPage("/album?id=3");
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.searchVisible.value, true);
  assert.equal(nav.searchLayer.value!.search!.query, "test");
});

test("a player's child retains the player without copying playback data", async () => {
  const { navigation: nav, router } = await fixture();
  await nav.openPlayer();
  const player = nav.current.value!.id;
  await nav.openPage("/album?id=4");
  assert.equal(nav.current.value!.parentId, player);
  assert.equal(nav.playerVisible.value, true);
  nav.finishTransition(nav.transition.value.id);
  assert.equal(nav.playerVisible.value, false);
  assert.equal(nav.hasLayer("player"), true);
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(nav.playerVisible.value, true);
  assert.equal(nav.current.value!.id, player);
});

test("root switches preserve the last category and stay parallel", async () => {
  const { navigation: nav } = await fixture();
  await nav.switchRoot("discover", "/discover/artists");
  const discover = nav.current.value!.id;
  await nav.openPage("/album?id=3");
  assert.equal(nav.current.value!.root, "discover");
  await nav.switchRoot("home", "/");
  await nav.switchRoot("discover", "/discover/playlists");
  assert.equal(nav.current.value!.id, discover);
  assert.equal(nav.current.value!.route, "/discover/artists");
  assert.equal(nav.state.value.layers.length, 1);
  assert.equal(nav.transition.value.direction, "root");
});

test("cancelled and redirected navigations never commit ghost layers", async () => {
  const { navigation: nav, router } = await fixture();
  const home = nav.current.value!.id;
  router.beforeEach((to) =>
    to.query.id === "cancel" ? false : to.query.id === "redirect" ? "/login" : undefined,
  );
  await nav.openPage("/album?id=cancel");
  assert.equal(nav.current.value!.id, home);
  await nav.openPage("/album?id=redirect");
  assert.equal(nav.current.value!.route, "/login");
  assert.equal(nav.state.value.layers.length, 2);
  assert.equal(nav.current.value!.source, undefined);
});

test("a superseded guard cannot overwrite the final navigation", async () => {
  const { navigation: nav, router } = await fixture();
  let release!: () => void;
  const waiting = new Promise<void>((resolve) => {
    release = resolve;
  });
  let entered!: () => void;
  const enteredGuard = new Promise<void>((resolve) => {
    entered = resolve;
  });
  router.beforeEach(async (to) => {
    if (to.query.id === "slow") {
      entered();
      await waiting;
    }
  });
  const slow = nav.openPage("/album?id=slow");
  await enteredGuard;
  await nav.openPage("/album?id=fast");
  release();
  await slow;
  assert.equal(nav.current.value!.route, "/album?id=fast");
  assert.equal(nav.state.value.layers.length, 2);
});

test("browser back and forward restore temporary layers", async () => {
  const { navigation: nav, router } = await fixture();
  await nav.openSearch();
  await traversed(router, () => router.back());
  assert.equal(nav.searchVisible.value, false);
  await traversed(router, () => router.forward());
  assert.equal(nav.searchVisible.value, true);
});

test("reload adopts serializable layers and preserves foreign history fields", async () => {
  const first = await fixture();
  first.history.replace("/", { foreign: "kept" });
  await first.navigation.openSearch();
  first.history.replace("/", { foreign: "kept" });
  first.navigation.updateSearchQuery("resume");
  const reloaded = await fixture("/", first.history.state);
  assert.equal(reloaded.navigation.searchVisible.value, true);
  assert.equal(reloaded.navigation.searchLayer.value!.search!.query, "resume");
  assert.equal(reloaded.history.state.foreign, "kept");
  assert.ok(readLayerHistory(reloaded.history.state));
});

test("deep links have an explicit fallback without invented source geometry", async () => {
  const { navigation: nav, router } = await fixture("/local/playlist?kind=local-album&album=A");
  assert.equal(nav.current.value!.root, "library");
  assert.equal(nav.current.value!.source, undefined);
  await traversed(router, () => {
    nav.closeTop();
  });
  assert.equal(router.currentRoute.value.path, "/local");
});

test("cache keys distinguish local collections and queries but not pagination", async () => {
  const { router } = await fixture();
  const key = (path: string) => pageCacheKey(router.resolve(path));
  assert.notEqual(
    key("/local/playlist?kind=local-album&album=A"),
    key("/local/playlist?kind=local-album&album=B"),
  );
  assert.notEqual(
    key("/local/playlist?kind=local-folder&sourceId=A&folder=x"),
    key("/local/playlist?kind=local-folder&sourceId=A&folder=y"),
  );
  assert.notEqual(key("/search/albums?keywords=A"), key("/search/albums?keywords=B"));
  assert.equal(key("/album?id=1&page=1"), key("/album?id=1&page=2"));
  assert.equal(key("/search/songs?keywords=A&page=1"), key("/search/albums?keywords=A&page=2"));
});

test("history rejects malformed state and view caches remain bounded", () => {
  assert.equal(readLayerHistory({ [HISTORY_KEY]: { version: 9 } }), undefined);
  const cache = new Map<number, string>();
  for (let i = 0; i < 80; i++) remember(cache, i, String(i));
  assert.equal(cache.size, 64);
  assert.equal(cache.has(0), false);
});
