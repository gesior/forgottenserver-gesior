# Compilation on Linux

There is no point in writing "how to compile on Linux" without being able to easily check if the instructions are up-to-date.
That's why Linux compilation instructions are in format of Docker files.

In the [dockerfiles](dockerfiles) folder are `Dockerfiles` with instructions on how to compile the engine on various Linux distributions.

On Linux, run only those lines that start with `RUN` in the `Dockerfile`. The other lines are instructions for Docker.

## Build using docker

**If you want to run the server using docker, use `Dockerfile` and `docker-compose.yaml` from the main project folder.**

_**TODO:** documentation on how docker-compose works on Windows (shared localhost etc.)._

### Test builds on Linux using docker

If you are working on Windows and want to check that the engine compiles on Linux after making changes, you can use the `Dockerfiles` and instructions described in [Linux Test Builds](linux-test-builds.md)
