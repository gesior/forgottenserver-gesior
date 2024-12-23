# Test builds using docker

The following commands compile OTS on all tested Linux distributions.
This is just a test to see if the engine compiles without warnings after changes.

In the main folder of the project, run:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_22_04 .
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 .
```

## Test builds with features enabled/disabled

Commands to run compilation with different compilation parameters on Linux Ubuntu 24.04.

### Debug

Debug, LUAJIT=OFF, UNITY_BUILD=OFF, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=OFF --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=OFF .
```

Debug, LUAJIT=OFF, UNITY_BUILD=OFF, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=OFF --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=ON .
```

Debug, LUAJIT=OFF, UNITY_BUILD=ON, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=OFF --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=OFF .
```

Debug, LUAJIT=OFF, UNITY_BUILD=ON, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=OFF --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=ON .
```

Debug, LUAJIT=ON, UNITY_BUILD=OFF, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=ON --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=OFF .
```

Debug, LUAJIT=ON, UNITY_BUILD=OFF, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=ON --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=ON .
```

Debug, LUAJIT=ON, UNITY_BUILD=ON, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=ON --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=OFF .
```

Debug, LUAJIT=ON, UNITY_BUILD=ON, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg BUILD_TYPE=Debug --build-arg USE_LUAJIT=ON --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=ON .
```

### RelWithDebInfo

RelWithDebInfo, LUAJIT=ON, UNITY_BUILD=OFF, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=ON  --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=OFF .
```

RelWithDebInfo, LUAJIT=ON, UNITY_BUILD=OFF, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=ON  --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=ON .
```

RelWithDebInfo, LUAJIT=ON, UNITY_BUILD=ON, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=ON  --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=OFF .
```

RelWithDebInfo, LUAJIT=ON, UNITY_BUILD=ON, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=ON  --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=ON .
```

RelWithDebInfo, LUAJIT=OFF, UNITY_BUILD=OFF, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=OFF  --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=OFF .
```

RelWithDebInfo, LUAJIT=OFF, UNITY_BUILD=OFF, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=OFF  --build-arg ENABLE_UNITY_BUILD=OFF --build-arg ENABLE_OTS_STATISTICS=ON .
```

RelWithDebInfo, LUAJIT=OFF, UNITY_BUILD=ON, OTS_STATS=OFF:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=OFF  --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=OFF .
```

RelWithDebInfo, LUAJIT=OFF, UNITY_BUILD=ON, OTS_STATS=ON:
```
docker build --progress=plain -f doc/compilation/linux/test-builds/Dockerfile_Ubuntu_24_04 --build-arg  BUILD_TYPE=RelWithDebInfo --build-arg USE_LUAJIT=OFF  --build-arg ENABLE_UNITY_BUILD=ON --build-arg ENABLE_OTS_STATISTICS=ON .
```
