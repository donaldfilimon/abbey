import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ErrorBoundary } from "../src/components/ErrorBoundary";

/**
 * React's server renderer never invokes error boundaries, so the boundary is
 * driven the way React drives it: the static `getDerivedStateFromError` turns
 * the thrown value into state, and `render` draws that state.
 */

function failed(thrown: unknown): string {
  const boundary = new ErrorBoundary({ children: <p>never shown</p> });
  boundary.state = { ...boundary.state, ...ErrorBoundary.getDerivedStateFromError(thrown) };
  return renderToStaticMarkup(boundary.render());
}

test("renders its children while nothing has failed", () => {
  const html = renderToStaticMarkup(
    <ErrorBoundary>
      <p>healthy view</p>
    </ErrorBoundary>,
  );
  expect(html).toContain("healthy view");
  expect(html).not.toContain("Recover interface");
});

test("renders the fallback with the error message and no stack in production mode", () => {
  const error = new Error("the surface could not parse its page");
  error.stack =
    "Error: the surface could not parse its page\n    at secretFrame (file:///private/source.tsx:12:3)";
  const html = failed(error);
  expect(html).toContain("This view needs a fresh start");
  expect(html).toContain("the surface could not parse its page");
  expect(html).not.toContain("secretFrame");
  expect(html).not.toContain("source.tsx");
  expect(html).not.toContain("never shown");
  expect(html).toContain("Recover interface");
});

test("a thrown non-Error value is still named, not dropped", () => {
  expect(failed("plain string failure")).toContain("plain string failure");
});
