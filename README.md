# Karta-rs
Karta, meaning map.

This is a small clipboard scanner meant to ease taking coordinates in the MMO Pantheon.

# User Guide
* TODO

## Goals
* Be a simple application for assisting players mapping the game world of Pantheon.
  * Write locations stored in clipboard to a ".csv" file to limit the manual steps needed.
* Be usable by a somewhat technical user and not just programmers.

# Developer Guide
## Development Build
To build during development, run:
```
cargo build
```

If it doesn't work, try:
```
cargo update
```

## Release Build
Longer build time, but more optimized and smaller executable.
```
cargo run --release
```

## Working on Update and Patches
* Always create a new branch from develop or directly in develop when working on a new version.
* When making a patch, make a branch directly from main.
* Use "-SNAPSHOT" at the end of a version that is not released. e.g. "1.1.0-SNAPSHOT" (found in `Cargo.toml`)

## Release
* "git pull" first to make sure no merge conflicts.
  ```sh
  git pull
  ```
* Have all features tested in develop
* Merge in develop into main
  ```sh
  git checkout main
  git merge develop
  ```
* Remove "-SNAPSHOT" from `Cargo.toml`.
* Build the project and verify that it still works.
* Update "Changelog.md".
* Commit as release.
* Tag the release and push it
  ```sh
  git tag -a 1.1.0 -m "release 1.1.0"
  git push origin 1.1.0
  ```
* Zip the built code in target, example "Karta_1.0.0.zip"
  * Create a release on Github and upload the zip.
* Merge changes to main back to develop
  ```sh
  git checkout develop
  git merge main
  git push
  ```
* Bump version in `Cargo.toml` and add back "-SNAPSHOT".
  * Commit changes and push.

## Patching
1. Create a branch from main.
  ```sh
  git checkout main
  git pull
  git checkout -b new-branch-name
  ```

2. Bump the patch number in version, `Cargo.toml`.
3. Fix the issue and test it.
4. Tag the new version and create a new release on Github.

## Versioning
* Major: Updated for major or breaking changes. Version example: 2.X.X
* Minor: Updated for normal features or changes. Version example: X.1.X
* Patch: Fix for an issue with a release. Version example: X.X.1
