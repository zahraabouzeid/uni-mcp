# Setup page

The page `unimcp` opens in the browser. This folder is the frontend (Next.js);
the backend is `unimcp` itself, which the page talks to through `/api/...`.
For a release the page is exported as static files and embedded into `unimcp`.

## Working on the page

In this folder:

```sh
npm run dev
```

It builds and starts the backend, starts the page with live reload, and opens
the browser. Saving a file in `web` updates the page; saving Rust code or a
profile rebuilds and restarts the backend, and the open page keeps working.
Stop everything with Ctrl+C.

`npm run dev:page` and `npm run dev:backend` start the two halves on their own.
The backend then prints the address to open; its key is new on every start.

## Shipping a change

```sh
npm run lint
npm run build       # writes out/
cd .. && cargo build
```

`cargo build` picks up the new `out/` by itself.
