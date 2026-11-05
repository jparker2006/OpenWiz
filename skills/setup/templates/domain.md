# Domain docs

Where this project keeps its words and its decisions, and how every skill reads them.

## Before exploring, read these

- **`GLOSSARY.md`** at the root: the project's words, each defined once. (In a repository with several separate packages, **`GLOSSARY-MAP.md`** at the root points at one `GLOSSARY.md` per package instead; read each one that bears on the work.)
- **`docs/adr/`**: one short file per hard-to-reverse decision. Read the ones that touch the area you are about to work in.
- **`docs/grills/`**: one file per grill, with every question and the user's exact answers, indexed in `docs/grills/README.md`. Read the grill that fed the spec you are working on.

If one of these does not exist yet, carry on without it. The `grill` skill creates them as terms and decisions get settled.

## Layout

```
/
├── GLOSSARY.md
├── docs/
│   ├── adr/
│   │   └── 0001-<decision>.md
│   └── grills/
│       ├── README.md
│       └── <date>-<topic>.md
└── ...
```

## Use the glossary's words

When your output names something the glossary defines (in an issue title, a ticket, a test name, a review), use the glossary's word. Avoid the words it lists under _Avoid_.

If what you need to name is not in the glossary yet, either you are inventing language the project doesn't use (think again), or there is a real gap: note it for the next grill.

## Flag ADR conflicts

If your output contradicts an ADR, say so plainly instead of quietly overriding it:

> _Contradicts ADR 0004 (<its title>), but worth reopening because…_
