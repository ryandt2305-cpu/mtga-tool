// Fixed-size virtual grid scroller. Renders only visible rows + overscan.
// Uses ResizeObserver to derive column count from container width.
// Caches item wrappers by source index so <For> sees stable references
// and doesn't remount DOM elements that are still visible after scroll.

import {
  createSignal,
  createMemo,
  onMount,
  onCleanup,
  For,
  type JSX,
} from "solid-js";

interface VirtualGridProps<T> {
  items: T[];
  itemWidth: number;
  itemHeight: number;
  gap: number;
  renderItem: (item: T, index: number) => JSX.Element;
}

interface VisibleEntry<T> {
  item: T;
  index: number;
  x: number;
  y: number;
}

const OVERSCAN_ROWS = 3;

export default function VirtualGrid<T>(props: VirtualGridProps<T>) {
  let containerRef!: HTMLDivElement;

  const [containerWidth, setContainerWidth] = createSignal(0);
  const [containerHeight, setContainerHeight] = createSignal(0);
  const [scrollTop, setScrollTop] = createSignal(0);

  const columns = createMemo(() => {
    const w = containerWidth();
    if (w <= 0) return 1;
    return Math.max(
      1,
      Math.floor((w + props.gap) / (props.itemWidth + props.gap))
    );
  });

  const totalRows = createMemo(() =>
    Math.ceil(props.items.length / columns())
  );

  const rowHeight = createMemo(() => props.itemHeight + props.gap);

  const totalHeight = createMemo(() => {
    const rows = totalRows();
    return rows > 0 ? rows * rowHeight() - props.gap : 0;
  });

  const visibleRange = createMemo(() => {
    const rh = rowHeight();
    if (rh <= 0) return { startRow: 0, endRow: 0 };
    const startRow = Math.max(
      0,
      Math.floor(scrollTop() / rh) - OVERSCAN_ROWS
    );
    const visibleRows = Math.ceil(containerHeight() / rh);
    const endRow = Math.min(
      totalRows(),
      Math.floor(scrollTop() / rh) + visibleRows + OVERSCAN_ROWS + 1
    );
    return { startRow, endRow };
  });

  // Cache item wrappers by source index. <For> uses referential identity —
  // reusing the same object reference for an unchanged item prevents remount.
  const entryCache = new Map<number, VisibleEntry<T>>();
  let cachedColumns = 0;

  const visibleItems = createMemo(() => {
    const { startRow, endRow } = visibleRange();
    const cols = columns();
    const rh = rowHeight();

    // Column count changed → positions are stale, clear cache
    if (cols !== cachedColumns) {
      entryCache.clear();
      cachedColumns = cols;
    }

    const result: VisibleEntry<T>[] = [];
    const activeIndices = new Set<number>();

    for (let row = startRow; row < endRow; row++) {
      for (let col = 0; col < cols; col++) {
        const index = row * cols + col;
        if (index >= props.items.length) break;
        activeIndices.add(index);

        const cached = entryCache.get(index);
        if (cached && cached.item === props.items[index]) {
          result.push(cached);
        } else {
          const entry: VisibleEntry<T> = {
            item: props.items[index],
            index,
            x: col * (props.itemWidth + props.gap),
            y: row * rh,
          };
          entryCache.set(index, entry);
          result.push(entry);
        }
      }
    }

    // Evict entries that scrolled out of view
    for (const key of entryCache.keys()) {
      if (!activeIndices.has(key)) entryCache.delete(key);
    }

    return result;
  });

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setContainerWidth(entry.contentRect.width);
        setContainerHeight(entry.contentRect.height);
      }
    });
    ro.observe(containerRef);
    onCleanup(() => ro.disconnect());
  });

  let rafId = 0;
  const onScroll = (e: Event) => {
    const target = e.target as HTMLDivElement;
    if (rafId) return; // Already scheduled
    rafId = requestAnimationFrame(() => {
      rafId = 0;
      setScrollTop(target.scrollTop);
    });
  };
  onCleanup(() => { if (rafId) cancelAnimationFrame(rafId); });

  return (
    <div
      ref={containerRef}
      style={{ width: "100%", height: "100%", overflow: "auto" }}
      onScroll={onScroll}
    >
      <div
        style={{
          position: "relative",
          height: `${totalHeight()}px`,
          "min-height": "1px",
        }}
      >
        <For each={visibleItems()}>
          {(entry) => {
            const col = entry.index % columns();
            const expand = props.itemHeight * 0.125;
            return (
              <div
                style={{
                  position: "absolute",
                  left: `${entry.x}px`,
                  top: `${entry.y}px`,
                  width: `${props.itemWidth}px`,
                  height: `${props.itemHeight}px`,
                }}
                data-grid-edge-left={col === 0 ? "" : undefined}
                data-grid-edge-right={col === columns() - 1 ? "" : undefined}
                data-grid-edge-top={entry.y - scrollTop() < expand ? "" : undefined}
                data-grid-edge-bottom={scrollTop() + containerHeight() - entry.y - props.itemHeight < expand ? "" : undefined}
              >
                {props.renderItem(entry.item, entry.index)}
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
