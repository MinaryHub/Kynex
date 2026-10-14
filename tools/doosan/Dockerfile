# run_drl, built with Doosan Robotics' DRFL (BSD 3-Clause) from
# doosan-robot2, at the commit pinned below.
#
#   docker build -t kynex-run-drl tools/doosan
#   docker run --rm --network container:kynex_emulator \
#       -v "$PWD/target/doosan:/work" kynex-run-drl 127.0.0.1 /work/stage1.drl
FROM ubuntu:22.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        g++ libpoco-dev ca-certificates curl && rm -rf /var/lib/apt/lists/*
ARG DRFL=https://raw.githubusercontent.com/DoosanRobotics/doosan-robot2/6c5f3ba622bfa9d6f9cffebf21fa44f57db55b48/dsr_common2
WORKDIR /drfl
RUN for f in DRFC.h DRFL.h DRFLEx.h DRFS.h; do curl -fsSL -o $f $DRFL/include/$f; done \
    && curl -fsSL -o libDRFL.a $DRFL/lib/humble/x86_64/libDRFL.a
COPY run_drl.cpp .
RUN g++ -O2 -std=c++17 -I. run_drl.cpp libDRFL.a -lPocoNet -lPocoFoundation -lpthread -o /usr/local/bin/run_drl
ENTRYPOINT ["run_drl"]
