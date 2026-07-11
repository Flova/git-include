<p align="center">
  <img src="docs/banner.svg" alt="git-include" width="720">
</p>

**[English](README.md)** | **Deutsch** | **[中文](README.zh-CN.md)**

`git-include` ist eine moderne Single-Binary-Alternative zu
[git-subrepo](https://github.com/ingydotnet/git-subrepo), geschrieben in Rust. Es
bindet ein Upstream-Repository als Unterverzeichnis in dein Repository ein, dazu
kommt eine kleine Marker-Datei. Das ist das gesamte Modell:

- **Mitwirkende brauchen nichts.** Sie machen `git clone` und haben lauffähigen
  Code. Kein `--recursive`, kein `submodule update`, keine git-include-Installation
  nötig. Nur wer mit Upstream synchronisiert, braucht das Tool.
- **Zwei-Wege-Synchronisation.** `git include pull` mischt neue Upstream-Arbeit in
  deinen Baum; `git include push` baut die Upstream-Historie aus deinen Commits neu
  auf — jeder Host-Commit, der das Verzeichnis verändert hat, wird zu einem
  eigenen Upstream-Commit mit Originalnachricht und -autor (auch Commits, die vor
  einem Pull gemacht wurden), und die Marker-Datei gelangt nie zu Upstream.
- **Kompatibel mit git-subrepo.** Die Marker-Datei nutzt dasselbe `.gitrepo`-Format.
  Ein Repository, das bereits git-subrepo verwendet, kann ohne Migration
  übernommen werden.
- **Export eingebaut.** `git include init` macht aus einem gewöhnlichen Verzeichnis
  ein neues eingebundenes Repository und extrahiert dabei dessen komplette
  Historie aus deinen Commits — bereit, in ein eigenes (auch leeres) Repository
  gepusht zu werden.
- Unkompliziertes **Branch-Wechseln**, schnelles **Status/Diff gegenüber Upstream**,
  **verschachtelte Includes** und **Tab-Vervollständigung** von Haus aus.

```console
$ git include add https://github.com/example/widgets vendor/widgets
$ git include status
$ git include pull vendor/widgets      # neue Upstream-Arbeit holen
$ git include push vendor/widgets      # eigene Änderungen zurück beitragen
```

---

## Inhaltsverzeichnis

- [Warum nicht Submodule / Subtree / Subrepo?](#warum-nicht-submodule--subtree--subrepo)
- [Installation](#installation)
- [Tab-Vervollständigung](#tab-vervollständigung)
- [Schnellstart](#schnellstart)
- [Befehlsübersicht](#befehlsübersicht)
- [Weg von Submodulen migrieren](#weg-von-submodulen-migrieren)
- [Auf Tags und Commits pinnen](#auf-tags-und-commits-pinnen)
- [Eigene Commit-Nachrichten](#eigene-commit-nachrichten)
- [Die `.gitrepo`-Marker-Datei](#die-gitrepo-marker-datei)
- [Git LFS](#git-lfs)
- [Ein Verzeichnis in ein eigenes Repository exportieren](#ein-verzeichnis-in-ein-eigenes-repository-exportieren)
- [Verschachtelte Includes](#verschachtelte-includes)
- [Merge-Konflikte behandeln](#merge-konflikte-behandeln)
- [Funktionsweise](#funktionsweise)
- [FAQ](#faq)
- [Entwicklung](#entwicklung)

---

## Warum nicht Submodule / Subtree / Subrepo?

Submodule lassen jeden Mitwirkenden bezahlen (zusätzliches Tooling, `--recursive`,
Detached-HEAD-Überraschungen); Subtree verschmutzt die Historie mit Merge-Rauschen
und versteckt seinen Zustand auf eine Weise, die schwer zu durchschauen ist; beide
machen alltägliche Aufgaben — „Diff gegen Upstream", „Tracked Branch wechseln",
„was ist noch nicht gepusht?" — umständlich oder unmöglich.

Die Grundidee ist dieselbe wie bei git-subrepo: **der eingebundene Code ist einfach
nur Dateien in deinem Repository**, und eine Marker-Datei speichert, woher sie
stammen und zu welchem Upstream-Commit sie gehören. Alles andere — Mergen,
Pushen, Diffen — wird daraus abgeleitet.

Im Vergleich zu git-subrepo ist git-include ein kompiliertes Binary (aufbauend auf
libgit2 über die `git2`-Crate) in Rust — einer stark typisierten Sprache mit
Compile-Zeit-Garantien — statt Bash, und legt nie temporäre Branches, Worktrees
oder Klone in deinem Repository an: deine Branches und dein Arbeitsverzeichnis
bleiben unangetastet, bis auf das eine bearbeitete Unterverzeichnis. Die CLI ist
intuitiver; das Pinnen auf einen bestimmten Tag oder Commit (nicht nur einen
Branch) wird unterstützt, ebenso wie Git LFS und die direkte Migration
bestehender Submodule.

## Installation

**Linux / macOS — Einzeiler:**

```console
$ curl -fsSL https://raw.githubusercontent.com/flova/git-include/main/install.sh | bash
```

Das Skript erkennt deine Plattform, lädt das aktuelle Release-Binary herunter,
verifiziert es gegen das `SHA256SUMS`-Manifest des Release und installiert es nach
`~/.local/bin` (oder `/usr/local/bin` als root). Für Linux werden zwei Varianten
veröffentlicht, zwischen denen das Skript automatisch wählt (überschreibbar mit
`GIT_INCLUDE_FLAVOR=dynamic|portable`):

- `*-linux-gnu` — dynamisch gegen die OpenSSL- und zlib-Bibliotheken der
  Distribution gelinkt; nichts mitgebracht. Bevorzugt, wenn das System kompatibel
  ist.
- `*-linux-gnu-portable` — OpenSSL und zlib **einkompiliert**; benötigt nur
  glibc ≥ 2.28 (2018), läuft also auf alten Distributionen und schlanken
  Container-Images ohne libssl. (Musl-basierte Distributionen wie Alpine
  bauen aus dem Quellcode — siehe unten.)

macOS-Binaries nutzen das systemeigene Security-Framework für TLS; OpenSSL wird
nur für SSH-Unterstützung einkompiliert (macOS liefert kein OpenSSL zum Linken
mit). Das Binary für die eigene Plattform lässt sich auch direkt von der
[Releases-Seite](https://github.com/flova/git-include/releases) herunterladen.
Eine Version festlegen mit `GIT_INCLUDE_VERSION=v0.1.0`, das Verzeichnis ändern
mit `GIT_INCLUDE_BIN_DIR`. Jederzeit aktualisieren — das Binary aktualisiert sich
selbst:

```console
$ git include self-update            # oder --version vX.Y.Z, oder -n zur Vorschau
```

(Self-Update-Downloads werden vor dem Ersetzen des laufenden Binaries gegen das
`SHA256SUMS`-Manifest des Release geprüft.)

(Self-Update ist nur in die Binaries einkompiliert, die git-include selbst
verteilt — die per curl installierten und der Windows-MSI-Installer.
Package-Manager-Builds wie conda deaktivieren es über ein Cargo-Feature-Flag.)

**Windows:** den MSI-Installer (x64) vom
[aktuellen Release](https://github.com/flova/git-include/releases/latest)
herunterladen — er installiert `git-include.exe` und legt es auf den `PATH`. Auf
ARM64-Windows stattdessen `git-include-aarch64-pc-windows-msvc.exe` aus den
Release-Assets holen und selbst auf den `PATH` legen. (`self-update`
funktioniert unter Windows ebenfalls, für beide Architekturen.)

**Conda:** jedes Release enthält `.conda`-Pakete für linux-64, linux-aarch64,
osx-arm64 und win-64 (siehe die Release-Assets; das Rezept liegt in
`conda/recipe.yaml`). Es gibt keine vorgefertigten Intel-Mac-Pakete oder
-Binaries — Intel-Mac-Nutzer bauen aus dem Quellcode (siehe unten). Conda-Builds
werden ohne den Self-Update-Mechanismus gebaut — dort ist Aktualisieren Aufgabe
von conda (`conda update git-include`), und `git include self-update` weist
entsprechend darauf hin, statt gegen den Package-Manager zu arbeiten.

**Aus dem Quellcode** (benötigt ein aktuelles stabiles Rust; libgit2 wird
mitgeliefert und einkompiliert, es gibt also außer OpenSSL unter Linux keine
Systemabhängigkeit):

```console
$ cargo install --git https://github.com/flova/git-include   # direkt von GitHub
$ cargo install --path .                                     # aus einem Checkout
```

Das Binary heißt `git-include`, git erkennt es also automatisch als Subcommand:
`git include <command>`. Prüfen mit:

```console
$ git include --version
```

## Tab-Vervollständigung

Ein Vervollständigungs-Skript für die eigene Shell erzeugen und aus der
Shell-Konfiguration einbinden:

```console
# bash — vervollständigt sowohl `git-include <TAB>` als auch `git include <TAB>`,
# inklusive Live-Vervollständigung eingebundener Verzeichnisse und Branch-Namen
$ git include completions bash > ~/.local/share/bash-completion/completions/git-include

# zsh — im eigenen $fpath ablegen; die git-Vervollständigung von zsh
# leitet automatisch weiter
$ git include completions zsh > ~/.zfunc/_git-include

# fish
$ git include completions fish > ~/.config/fish/completions/git-include.fish
```

Elvish und PowerShell werden ebenfalls unterstützt (`git include completions --help`).

## Schnellstart

### Ein Repository einbinden

```console
$ git include add https://github.com/example/widgets vendor/widgets
No branch given; using upstream default branch 'main'.
Fetching https://github.com/example/widgets (main) ...
Added 'vendor/widgets' from https://github.com/example/widgets (branch main, commit 1a2b3c4).
```

Das erzeugt **einen Commit** in deinem Repository mit dem vollständigen
Upstream-Baum unter `vendor/widgets/` plus `vendor/widgets/.gitrepo`. Von da an
ist das Verzeichnis vollkommen gewöhnlich: bearbeiten, committen, zurücksetzen,
durch die Historie bisecten — es sind einfach Dateien.

### Den eigenen Stand sehen

```console
$ git include status --fetch
vendor/widgets
  remote:   https://github.com/example/widgets
  branch:   main (synced at 1a2b3c4)
  upstream: 2 new commit(s) available -> `git include pull vendor/widgets`
  local:    1 commit(s) to push -> `git include push vendor/widgets`

$ git include diff vendor/widgets              # eigene Änderungen seit letzter Synchronisation
$ git include diff vendor/widgets --upstream --fetch   # gegen den aktuellen Upstream-Stand
```

Die `diff`-Ausgabe wird wie `git diff` eingefärbt, wenn sie in ein Terminal
geschrieben wird (deaktivierbar mit der Standardumgebungsvariable `NO_COLOR`).

Ohne `--fetch` verwendet `status` den Upstream-Stand des letzten Fetches, ist also
sofort da und funktioniert offline.

### Upstream-Änderungen holen

```console
$ git include pull vendor/widgets
```

Eigene Änderungen am Verzeichnis (falls vorhanden) werden per Drei-Wege-Merge mit
den Upstream-Änderungen zusammengeführt, genau wie bei einem `git merge` —
inklusive inhaltlicher Merges und Konfliktmarkern, wenn beide Seiten dieselben
Zeilen verändert haben. Das Ergebnis ist ein einzelner Commit im eigenen
Repository. `git include pull --all` synchronisiert jedes eingebundene
Verzeichnis; bei nur einem Include reicht ein einfaches `git include pull`.

Ist der lokale Stand des Verzeichnisses nicht mehr zu retten, verwirft `git
include pull --force` ihn — committet oder nicht — und übernimmt Upstream
unverändert. Verworfene Änderungen werden auch von künftigen Pushes
ausgeschlossen.

### Eigene Änderungen zu Upstream pushen

```console
$ git include push vendor/widgets
Pushed 2 commit(s) from 'vendor/widgets' to https://github.com/example/widgets (main); upstream is now 9f8e7d6.
```

`push` baut die Upstream-Historie als **1:1-Abbild der eigenen Host-Commits** neu
auf: jeder Commit, der das Verzeichnis seit der letzten Übernahme nach Upstream
verändert hat, wird zu einem eigenen Upstream-Commit — Originalnachricht,
Originalautor, nur mit den Dateien des Verzeichnisses. Branches und Merges werden
exakt so gespiegelt, wie sie im Host-Repository stattgefunden haben (ein
Host-Merge, der widersprüchliche Branch-Änderungen aufgelöst hat, kommt als
derselbe Merge-Commit mit derselben Auflösung an); Commits, die das Verzeichnis
nie berührt haben, bleiben außen vor. Das funktioniert **über Pulls hinweg**:
Commits, die vor einem Pull gemacht wurden, bleiben eigenständige Commits,
basierend auf dem Upstream-Stand, gegen den sie tatsächlich geschrieben wurden,
und der Pull selbst wird zu einem gewöhnlichen Merge mit der eigenen Historie von
Upstream. Die Commit-Hashes unterscheiden sich zwangsläufig von den
Host-Commits, aber Inhalt und Topologie bleiben exakt erhalten. Der
`.gitrepo`-Marker wird automatisch entfernt und taucht nie bei Upstream auf.

Vorschau mit `git include push -n <dir>`; `--squash` verwenden, um stattdessen
alles als einen einzigen Commit zu veröffentlichen.

Hat sich Upstream in der Zwischenzeit bewegt, verweigert `push` und bittet
zunächst um `git include pull`, damit Upstream nie ein überraschendes
Merge-Ergebnis bekommt.

Pushes können auch einen **anderen Branch und/oder ein anderes Remote** als Ziel
haben — einen Feature-Branch, oder einen Fork:

```console
$ git include push vendor/widgets --branch feature/my-fix
$ git include push vendor/widgets --remote git@github.com:me/widgets-fork -b pr/fix --keep
```

Standardmäßig wird das Include auf das Ziel des Pushs **umgestellt** (der Marker
speichert das neue Remote/den neuen Branch, künftige Pulls folgen dem). Mit
`--keep` für den Temporär-Fork-Workflow: der Push findet statt, aber der Marker
verfolgt weiterhin den ursprünglichen Stand — sobald der Vorschlag bei Upstream
gemerged ist, holt ihn ein normaler `pull`. Beides funktioniert auch mit einem auf
einen Tag oder Commit gepinnten Include (`--branch` benennt dann das Ziel). Ein
bereits existierender Ziel-Branch wird nur an der aufgezeichneten Basis
akzeptiert, sodass fremde Arbeit nie überschrieben wird.

`pull` und `switch` akzeptieren ebenfalls `--remote <url>` — ein Pull stellt den
Marker immer auf das Remote um, von dem gepullt wurde. Das macht `pull --remote`
auch zum Weg, einem umgezogenen Upstream zu folgen: ein Pull von der neuen
Adresse stellt das Include um, selbst wenn sich der Inhalt nicht ändert.

### Den getrackten Branch wechseln

```console
$ git include branches vendor/widgets
* main (1a2b3c4)
  next (5d6e7f8)

$ git include switch vendor/widgets next
Switched 'vendor/widgets' to branch next (commit 5d6e7f8).
```

Beim Wechseln werden lokale Änderungen übernommen (gemerged); ein sauberes
Verzeichnis wird einfach durch den Inhalt des neuen Branches ersetzt. Zurück
wechseln funktioniert mit demselben Befehl. `switch` akzeptiert auch einen Tag
oder eine Commit-ID — siehe
[Auf Tags und Commits pinnen](#auf-tags-und-commits-pinnen).

## Befehlsübersicht

| Befehl | Beschreibung |
| --- | --- |
| `git include add <remote> <dir> [-b <branch> \| -t <tag> \| --commit <sha>]` | Ein Upstream-Repository nach `<dir>` einbinden, mit Tracking eines Branches (Standard: der Standard-Branch des Remote) oder gepinnt auf einen Tag/Commit. |
| `git include pull [<dir>] [--all] [--force] [-r <url>]` | Neue Upstream-Commits in `<dir>` mergen (oder in alle Includes); `--force` verwirft lokale Änderungen, `-r` pullt von (und stellt um auf) ein anderes Remote. |
| `git include push <dir> [-n] [-b <branch>] [-r <url>] [--keep] [--squash]` | Lokale Commits, die `<dir>` betreffen, auf den Upstream-Branch übertragen und pushen; `-b`/`-r` pushen (und stellen um) woandershin, `--keep` behält das aktuelle Tracking bei. |
| `git include status [<dir>] [-f/--fetch]` | Sync-Status anzeigen: bei Upstream verfügbare Commits, zu pushende Commits, uncommittete Änderungen. |
| `git include diff <dir> [--upstream] [--stat] [-f/--fetch]` | `<dir>` gegen den zuletzt synchronisierten Commit vergleichen, oder gegen den aktuellen Upstream-Stand. |
| `git include switch <dir> <branch\|tag\|commit>` `[-r <url>]` | Einen anderen Branch tracken, oder auf einen Tag/Commit pinnen, lokale Änderungen werden übernommen; `-r` wechselt auch das Remote. |
| `git include branches <dir>` | Upstream-Branches und -Tags auflisten, den getrackten Stand markiert. |
| `git include migrate [<path>...]` | Git-Submodule in Includes umwandeln — alle, oder nur die angegebenen Pfade. |
| `git include list` | Alle Includes auflisten, verschachtelte eingerückt. |
| `git include remove <dir>` | Ein Include aus dem Arbeitsverzeichnis löschen (Historie und Upstream bleiben unangetastet). |
| `git include completions <shell>` | Ein Tab-Vervollständigungs-Skript ausgeben. |
| `git include self-update [--version <tag>]` | Das git-include-Binary auf das aktuelle (oder ein bestimmtes) Release aktualisieren. |

Alle `<dir>`-Argumente sind relativ zum aktuellen Verzeichnis, die Befehle
funktionieren also von überall im Repository aus. `--no-lfs` wird von `add`,
`pull`, `push` und `switch` akzeptiert, um LFS-Transfers zu überspringen;
`-m/--message` wird von jedem Befehl akzeptiert, der einen Sync-Commit erzeugt
(siehe [Eigene Commit-Nachrichten](#eigene-commit-nachrichten)).

## Weg von Submodulen migrieren

Ein Befehl macht aus einem submodul-basierten Repository ein include-basiertes:

```console
$ git include migrate                # jedes Submodul konvertieren
$ git include migrate vendor/lib     # oder nur dieses eine
Migrating submodule 'vendor/lib' (recorded commit 1a2b3c4) ...
Migrated 'vendor/lib' -> include of https://github.com/example/lib pinned to commit 1a2b3c4.
```

Jedes Submodul wird zu einem Include, **gepinnt genau auf den Commit, den das
Submodul verzeichnet hatte**, sodass die Migration den Inhalt des Baums nie
verändert — ein Commit pro Submodul, der den Gitlink in gewöhnliche Dateien mit
einem `.gitrepo`-Marker umwandelt. `.gitmodules`-Einträge werden entfernt (die
Datei wird gelöscht, sobald sie leer ist), und der übrig gebliebene
`.git/modules`-Klon des Submoduls sowie die `submodule.*`-Konfiguration werden
aufgeräumt. Danach lässt sich jedes Include von seiner Pinnung auf einen
lebenden Branch umstellen mit `git include switch <dir> <branch>`.

## Auf Tags und Commits pinnen

Anders als git-subrepo muss ein Include keinen Branch tracken — es kann auch auf
einen exakten Upstream-Stand gepinnt werden:

```console
$ git include add https://github.com/example/widgets vendor/widgets --tag v2.1.0
$ git include add https://github.com/example/parser  vendor/parser  --commit 9f8e7d6c...
$ git include switch vendor/widgets v2.2.0     # zwischen Releases wechseln
$ git include switch vendor/widgets main       # zurück zum Branch-Tracking
```

`switch` löst sein Argument automatisch auf (zuerst Branch, dann Tag, dann
Commit-ID), sodass der Wechsel zwischen Releases und Branch-Tracking in beiden
Richtungen ein einziger Befehl ist. Ein gepinntes Include ist vollständig
reproduzierbar: `pull` meldet die Pinnung, statt sich zu bewegen, `status`/`diff`
vergleichen gegen den gepinnten Stand, und `push` verweigert mit einem Hinweis
auf `switch` (es gibt keinen Branch, zu dem gepusht werden könnte). Lokale
Änderungen werden beim Wechseln übernommen — oder mit `switch --force`
verworfen.

## Eigene Commit-Nachrichten

Die Nachrichten der Sync-Commits, die git-include erzeugt (add, pull, switch,
push-Buchführung, init, remove), sind mit Jinja templatebar (über
[minijinja](https://crates.io/crates/minijinja)) — Variablen, Filter und
Bedingungen funktionieren alle:

```console
# pro Repository (oder --global), für alle Sync-Commits:
$ git config include.commitTemplate 'chore(vendor): {{ action }} {{ subdir }} @ {{ short_commit }}'

# oder pro Aufruf:
$ git include pull vendor/widgets -m 'vendor: update widgets to {{ short_commit }}'

# vollständige Jinja-Ausdrücke sind verfügbar:
$ git include pull vendor/widgets \
    -m '{% if action == "pull" %}⬆{% endif %} {{ subdir | upper }} @ {{ short_commit }}'
```

| Variable | Wert |
| --- | --- |
| `{{ action }}` | der Befehl, inklusive relevanter Flags (z. B. `pull --force`) |
| `{{ subdir }}` | das eingebundene Verzeichnis |
| `{{ remote }}` | die Upstream-URL |
| `{{ ref }}` (Alias `{{ branch }}`) | der getrackte Branch/Tag/Commit |
| `{{ ref_kind }}` | `branch`, `tag` oder `commit` |
| `{{ commit }}` / `{{ short_commit }}` | der Upstream-Commit (vollständig / 7 Zeichen) |
| `{{ version }}` | die git-include-Version |

Die literale Sequenz `\n` wird zu einem Zeilenumbruch, sodass mehrzeilige
Nachrichten in einen einzeiligen Config-Wert passen. Ein fehlerhaftes Template
(Syntaxfehler oder unbekannte Variable) gibt eine Warnung aus und fällt auf die
Standardnachricht zurück — eine abgeschlossene Synchronisation wird nie wegen
eines Tippfehlers abgebrochen. Ohne Template schreibt git-include seine
strukturierte Standardnachricht (`git include <action> <dir>` plus einen
Metadaten-Block).

## Die `.gitrepo`-Marker-Datei

Jedes eingebundene Verzeichnis enthält eine `.gitrepo`-Datei im Format von
git-subrepo:

```ini
; DO NOT EDIT (unless you know what you are doing)
;
[subrepo]
	remote = https://github.com/example/widgets
	branch = main
	commit = 1a2b3c4d...   ; upstream commit the directory was last synced to
	parent = 9z8y7x6w...   ; last host commit whose changes are already upstream
	method = merge
	cmdver = 0.1.0
```

Da Format, Schlüssel und Semantik mit git-subrepo übereinstimmen, erfordert die
Einführung von git-include in einem bestehenden git-subrepo-Projekt keinerlei
Migration: es arbeitet direkt mit Verzeichnissen, die mit `git subrepo clone`
eingebunden wurden. Die umgekehrte Richtung funktioniert für
Branch-trackende Includes, aber Achtung: git-subrepo kennt kein Pinnen auf einen
Tag oder Commit — ein Include, das diese Funktionen nutzt, hat keine
Entsprechung in git-subrepo.

## Git LFS

Wenn das Upstream-Repository Git LFS verwendet, bemerkt git-include das (über
`filter=lfs` in dessen `.gitattributes`) und behandelt es automatisch:

- **add / pull / switch** holen die LFS-Objekte aus dem *Upstream*-LFS-Store und
  materialisieren echten Inhalt im Arbeitsverzeichnis,
- **push** lädt LFS-Objekte, auf die die eigenen Commits verweisen, *vor* dem
  Pushen der Git-Objekte hoch, sodass Upstream nie hängende Pointer sieht,
- ist `git-lfs` nicht installiert, gelingen die Operationen trotzdem — es gibt
  dann Pointer-Dateien plus eine klare Warnung mit den genauen Befehlen, die
  später auszuführen sind,
- `--no-lfs` überspringt das alles.

## Ein Verzeichnis in ein eigenes Repository exportieren

Die Umkehrung von `add`: ein Verzeichnis, das innerhalb des eigenen Repositorys
gewachsen ist, kann zu einem eigenen Repository aufsteigen, samt Historie.

```console
$ git include init mylib --remote git@github.com:me/mylib.git
Extracting the history of 'mylib' ...
Turned 'mylib' into an included repository: extracted 17 commit(s) of history (head 3fc9a21).
Publish it with: git include push mylib

$ git include push mylib
Published 'mylib' to git@github.com:me/mylib.git as new branch 'main'.
```

`init` (Alias: `export`) durchläuft die gesamte eigene Historie, und jeder
Commit, der das Verzeichnis verändert hat, wird zu einem Commit einer
brandneuen, eigenständigen Historie — Originalautor und -nachricht, Inhalt
gefiltert auf das Verzeichnis (ein Commit, der sowohl `mylib/` als auch andere
Dateien betroffen hat, trägt nur seinen `mylib/`-Teil bei). `push` veröffentlicht
diese Historie dann und legt bei Bedarf den Branch auf einem leeren Remote an.
Von diesem Moment an ist das Verzeichnis ein normales Include: andere können es
mit `git include add` einbinden, und `pull`/`push`/`status` funktionieren wie
gewohnt.

## Verschachtelte Includes

Eingebundene Repositories können selbst wieder Includes enthalten. Da alles
einfach nur Dateien sind, wandern die inneren `.gitrepo`-Marker automatisch mit:

```console
$ git include add https://github.com/example/app libs/app
$ git include list
libs/app  <-  https://github.com/example/app (main @ 4ee9c11)
  libs/app/vendor/parser  <-  https://github.com/example/parser (main @ 77af0d3)
```

Es lässt sich auf jeder Ebene arbeiten: `git include pull libs/app`
synchronisiert das äußere Repository (und bringt mit, welchen Stand von
`vendor/parser` es committet hat), während `git include pull
libs/app/vendor/parser` das innere direkt von *seinem* Upstream synchronisiert.
Beim Pushen eines Includes wird nur dessen eigener Marker entfernt —
verschachtelte Marker sind Inhalt und werden unverändert mitgepusht.

## Merge-Konflikte behandeln

Wenn sowohl die eigene Seite als auch Upstream dieselben Zeilen geändert haben,
stoppt `pull` mit den konfliktbehafteten Dateien im Arbeitsverzeichnis, versehen
mit den üblichen Konfliktmarkern:

```console
$ git include pull vendor/widgets
CONFLICT: could not automatically merge upstream changes into 'vendor/widgets'.
Files with conflict markers:
  vendor/widgets/src/lib.rs

Resolve the conflicts, then finish with:
  git add vendor/widgets
  git commit
```

Es gibt keinen speziellen „Continue"-Zustand zu verwalten: Marker auflösen, `git
add`, `git commit` — fertig. (Die `.gitrepo`-Aktualisierung ist bereits für dich
vorbereitet.) Um stattdessen abzubrechen, stellt `git reset --hard` den Stand vor
dem Pull wieder her.

## Funktionsweise

Jede Operation ist eine reine Funktion der vier Marker-Werte (`remote`,
`branch`, `commit`, `parent`) plus dem aktuellen Zustand des Host-Repositorys
und des Upstream-Remotes — es gibt keinen Zustand in `.git/config`, keine
registrierten Remotes, keine temporären Branches. Alles läuft in-process über
libgit2 (die `git2`-Crate):

- `add` holt den Upstream-Branch und pfropft dessen Baum unter das Präfix, indem
  der Root-Baum umgeschrieben wird — ein Commit, keine gemeinsame Historie mit
  Upstream.
- `pull` nimmt drei Bäume — den Baum des zuletzt synchronisierten
  Upstream-Commits (Basis), den aktuellen eigenen Verzeichnisbaum (unsere Seite)
  und den Baum des neuen Upstream-Head (deren Seite) — und übergibt sie dem
  Drei-Wege-Merge von libgit2 (inklusive Umbenennungserkennung). Ein sauberer
  Merge wird zu einem einzelnen Host-Commit; Konflikte werden im
  Arbeitsverzeichnis mit den üblichen Konfliktmarkern materialisiert.
- `push` prüft zunächst, ob der Upstream-Branch noch auf der aufgezeichneten
  Basis steht (damit das Ergebnis ein reiner Fast-Forward bei Upstream ist, nie
  ein überraschender Merge), und bildet dann jeden Host-Commit, der das
  Verzeichnis verändert hat, auf einen Upstream-Commit ab — Unterverzeichnisbaum
  wörtlich übernommen mit entferntem Marker, Originalnachricht und -autor, die
  Host-Eltern-Commits übersetzt auf ihre eigenen Upstream-Abbilder, sodass
  Branching und Mergen unverändert erhalten bleiben. Reine
  Marker-Buchführungs-Commits werden automatisch übersprungen, und
  Sync-Commits werden auf den Upstream-Commit abgebildet, den sie übernommen
  haben (ein Pull, der lokale Arbeit gemerged hat, wird zu einem echten Merge
  mit Upstream). Nur der *eigene* Marker des Includes wird entfernt;
  verschachtelte `.gitrepo`-Dateien sind Inhalt und reisen unverändert mit nach
  Upstream.
- Geholte Upstream-Stände werden unter `refs/include/<dir>` gepinnt, sodass
  `status` und `diff` offline funktionieren und geholte Objekte ein `git gc`
  überleben.

Ein subtiler Fall wird explizit behandelt: ein frischer Klon des
Host-Repositorys hat die eingebundenen *Bäume und Blobs* (sie sind von
Host-Commits aus erreichbar), aber nicht die Upstream-*Commit*-Objekte.
Sync-Befehle holen daher bei Bedarf vom Upstream-Remote nach und erkennen
umgeschriebene Upstream-Historie (Force-Pushes) mit einem klaren
Wiederherstellungsweg, statt einen unsinnigen Merge zu erzeugen.

Keine temporären Branches, kein `.git/modules`, kein Stashing, kein Antasten des
Arbeitsverzeichnisses außerhalb des eingebundenen Verzeichnisses — und anders
als git-subrepo keine Abhängigkeit von `git subtree`-artiger
Squash-Merge-Maschinerie.

## FAQ

**Brauchen meine Mitwirkenden git-include?**
Nein. Das eingebundene Verzeichnis besteht aus gewöhnlichen Dateien. Nur wer
`pull`/`push`/`switch` ausführt, braucht das Tool.

**Bläht `add` mein Repository auf?**
Es kommt der Upstream-*Baum* (eine Momentaufnahme) in den eigenen Branch, nicht
dessen Historie. Die geholte Upstream-Historie bleibt im lokalen
Objekt-Store für das Mergen, wird aber nie zum eigenen Host-Remote gepusht.

**Kann ich eingebundene Dateien direkt bearbeiten?**
Ja — genau darum geht es. Ganz normal committen; `git include status` zeigt, was
noch nicht zu Upstream gepusht wurde.

**Was, wenn Upstream force-gepusht hat?**
`pull` und `push` erkennen, dass der aufgezeichnete Commit bei Upstream nicht
mehr existiert, und sagen, wie man sich davon erholt.

**Welche Git-Version brauche ich?**
Egal welche — git-include bringt libgit2 mit und spricht selbst mit Remotes, es
funktioniert also unabhängig von der auf der Maschine installierten
Git-Version. Die einzige optionale externe Abhängigkeit ist `git-lfs` für
LFS-Inhalte, und die eigenen Zugangsdaten werden auf dem üblichen Weg
übernommen (ssh-agent und Git-Credential-Helper).

## Entwicklung

Die Entwicklungs- und Release-Umgebung ist mit [pixi](https://pixi.sh) gepinnt —
ein Befehl liefert genau die Rust-Toolchain, git-lfs, den C-Compiler und
rattler-build, mit denen das Projekt gebaut und getestet wird (Versionen
festgeschrieben in `pixi.lock`):

```console
$ pixi run test               # vollständige Testsuite, inklusive LFS-Roundtrip
$ pixi run lint               # rustfmt + clippy, genau wie in CI
$ pixi run build              # Release-Binary für die eigene Plattform
$ pixi run -e build conda-build   # das Conda-Paket bauen
```

Die Testsuite ist umfangreich: sie prüft Zwei-Wege-Synchronisation,
verschachtelte Includes, Git LFS, Submodul-Migration und Grenzfälle wie
widersprüchliche parallele Branches Ende-zu-Ende gegen echte Git-Repositories,
und läuft in CI bei jeder Änderung.

Es gibt kein separates Toolchain-Setup — Entwicklung, CI und Releases laufen
alle über pixi. Releases werden von CI aus einem `v*`-Tag gebaut, vollständig
mit der pixi-gepinnten Toolchain (der `dist`-Umgebung — kein rustup, keine
Systempakete); der Release-Workflow lässt sich auch manuell als Trockenlauf
auslösen, der alle Artefakte erzeugt, ohne etwas zu veröffentlichen.

## Lizenz

MIT — siehe [LICENSE](LICENSE).
