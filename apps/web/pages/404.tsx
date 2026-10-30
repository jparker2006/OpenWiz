// The static export's 404 page, in the app's own style. Next's default one
// carries inline styles, which the page CSP refuses (V3).
export default function NotFound() {
  return (
    <main>
      <h1 className="disp m">Not found</h1>
      <p className="body">
        <a href="/">Sessions</a>
      </p>
    </main>
  );
}
