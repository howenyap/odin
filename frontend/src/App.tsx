import { useCallback, useEffect, useMemo, useState } from "react";
import type { ChangeEvent } from "react";
import { useQuery } from "@tanstack/react-query";

const API_BASE = "http://localhost:3000";

type SearchResultItem = {
  url: string;
  title?: string | null;
  excerpt?: string | null;
  score: number;
};

type SearchResponse = {
  total_hits: number;
  results: SearchResultItem[];
};

const get_hostname = (url: string) => {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
};

export default function App() {
  const [draft_query, set_draft_query] = useState("");
  const [active_query, set_active_query] = useState("");

  const trimmed_draft_query = useMemo(() => draft_query.trim(), [draft_query]);

  const update_url_query = useCallback((next_query: string) => {
    const params = new URLSearchParams(window.location.search);

    if (next_query) {
      params.set("q", next_query);
    } else {
      params.delete("q");
    }

    const next_search = params.toString();
    const next_url = next_search
      ? `${window.location.pathname}?${next_search}`
      : window.location.pathname;

    window.history.pushState({}, "", next_url);
  }, []);

  const { data, error, isError, isFetching, isSuccess } = useQuery({
    queryKey: ["search", active_query],
    queryFn: async ({ queryKey, signal }) => {
      const [, query] = queryKey as [string, string];

      if (!query) {
        return { total_hits: 0, results: [] } satisfies SearchResponse;
      }

      const url = `${API_BASE}/v1/search?q=${encodeURIComponent(query)}`;
      const response = await fetch(url, { signal });

      if (!response.ok) {
        throw new Error(`Request failed with ${response.status}`);
      }

      return (await response.json()) as SearchResponse;
    },
    enabled: active_query.length > 0,
    placeholderData: (previous_data) => previous_data,
  });

  const results = data?.results ?? [];
  const total_hits = data?.total_hits ?? 0;
  const error_message = error instanceof Error ? error.message : "Unknown error";

  const apply_query_from_url = useCallback(() => {
    const params = new URLSearchParams(window.location.search);
    const next_query = (params.get("q") ?? "").trim();

    set_draft_query(next_query);
    set_active_query(next_query);
  }, []);

  useEffect(() => {
    apply_query_from_url();
  }, [apply_query_from_url]);

  useEffect(() => {
    const handle_popstate = () => {
      apply_query_from_url();
    };

    window.addEventListener("popstate", handle_popstate);
    return () => window.removeEventListener("popstate", handle_popstate);
  }, [apply_query_from_url]);

  const show_results = active_query.length > 0;
  const title = show_results ? `About ${total_hits} results` : "Results";

  return (
    <div className="min-h-screen bg-stone-50 text-stone-900">
      {show_results ? (
        <>
          <header className="border-b border-stone-200 bg-white px-4 py-3">
            <div className="mx-auto flex max-w-2xl items-center gap-3">
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  set_draft_query("");
                  set_active_query("");
                  update_url_query("");
                }}
                className="shrink-0 text-2xl font-medium text-stone-800 hover:text-stone-600"
              >
                Odin
              </a>
              <form
                className="min-w-0 flex-1"
                onSubmit={(event) => {
                  event.preventDefault();
                  set_active_query(trimmed_draft_query);
                  update_url_query(trimmed_draft_query);
                }}
              >
                <div className="flex w-full items-center rounded-full border border-stone-300 bg-stone-50 px-4 py-2 focus-within:border-stone-400 focus-within:ring-1 focus-within:ring-stone-300">
                  <span className="mr-2 text-stone-500">
                    <svg viewBox="0 0 24 24" className="size-5">
                      <circle
                        cx="11"
                        cy="11"
                        r="7"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                      />
                      <path
                        d="M16.5 16.5L21 21"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </svg>
                  </span>
                  <input
                    type="text"
                    className="min-w-0 flex-1 bg-transparent text-stone-900 placeholder-stone-400 outline-none"
                    placeholder="Search the web..."
                    value={draft_query}
                    onChange={(event: ChangeEvent<HTMLInputElement>) =>
                      set_draft_query(event.target.value)
                    }
                  />
                  <button
                    type="button"
                    onClick={() => set_draft_query("")}
                    className={`rounded p-1 text-stone-500 hover:bg-stone-200 hover:text-stone-700 ${
                      draft_query ? "" : "pointer-events-none opacity-0"
                    }`}
                    aria-label="Clear search"
                    tabIndex={draft_query ? 0 : -1}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      aria-hidden="true"
                      className="size-5"
                    >
                      <path
                        d="M6 6l12 12M18 6l-12 12"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </svg>
                  </button>
                </div>
              </form>
            </div>
          </header>
          <section className="mx-auto max-w-2xl px-4 py-6">
            <div className="mb-4 text-sm text-stone-500">{title}</div>
            <div className="space-y-6">
              {isFetching ? (
                <div className="py-8 text-center text-stone-500">
                  Searching...
                </div>
              ) : null}
              {isError ? (
                <div className="rounded border border-red-200 bg-red-50 px-4 py-3 text-red-800">
                  Search failed: {error_message}
                </div>
              ) : null}
              {isSuccess && results.length === 0 ? (
                <div className="py-8 text-center text-stone-500">
                  No results yet.
                </div>
              ) : null}

              {results.map((result) => (
                <article key={result.url} className="group">
                  <a
                    href={result.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="block"
                  >
                    <div className="mb-0.5 text-xs text-stone-500 group-hover:text-stone-700">
                      {get_hostname(result.url)}
                    </div>
                    <div className="text-lg font-medium text-stone-900 group-hover:underline">
                      {result.title?.trim() || "Untitled"}
                    </div>
                  </a>
                </article>
              ))}
            </div>
          </section>
        </>
      ) : (
        <section className="flex min-h-screen flex-col items-center justify-center px-4">
          <div className="flex w-full max-w-xl flex-col items-center gap-8">
            <h1 className="text-8xl font-medium text-stone-800">Odin</h1>
            <form
              className="w-full"
              onSubmit={(event) => {
                event.preventDefault();
                set_active_query(trimmed_draft_query);
                update_url_query(trimmed_draft_query);
              }}
            >
              <div className="flex items-center rounded-full border border-stone-300 bg-white px-4 py-3 shadow-sm focus-within:border-stone-400 focus-within:ring-2 focus-within:ring-stone-200">
                <span className="mr-3 text-stone-500">
                  <svg viewBox="0 0 24 24" className="size-5">
                    <circle
                      cx="11"
                      cy="11"
                      r="7"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                    />
                    <path
                      d="M16.5 16.5L21 21"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                    />
                  </svg>
                </span>
                <input
                  type="text"
                  className="min-w-0 flex-1 bg-transparent text-stone-900 placeholder-stone-400 outline-none"
                  placeholder="Search the web..."
                  value={draft_query}
                  onChange={(event: ChangeEvent<HTMLInputElement>) =>
                    set_draft_query(event.target.value)
                  }
                />
              </div>
            </form>
          </div>
        </section>
      )}
    </div>
  );
}
