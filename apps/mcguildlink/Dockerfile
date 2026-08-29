FROM --platform=$BUILDPLATFORM eclipse-temurin:25-jdk AS builder
WORKDIR /workspace

COPY apps/mcguildlink/gradlew ./
COPY apps/mcguildlink/gradle/ ./gradle/
COPY apps/mcguildlink/settings.gradle.kts ./
COPY apps/mcguildlink/gradle.properties ./
COPY apps/mcguildlink/app/build.gradle.kts ./app/build.gradle.kts

RUN ./gradlew \
    --no-daemon \
    :app:dependencies \
    --configuration runtimeClasspath

COPY apps/mcguildlink/app/src/ ./app/src/

RUN ./gradlew \
    --no-daemon \
    --no-configuration-cache \
    installDist generateLicenseReport && \
    mkdir -p /workspace/runtime/config \
             /workspace/runtime/data \
             /workspace/runtime/static

FROM eclipse-temurin:25-jdk AS runtime-builder
RUN jlink \
    --add-modules java.base,java.net.http,java.sql,jdk.unsupported \
    --strip-debug \
    --no-man-pages \
    --no-header-files \
    --compress=zip-9 \
    --output /runtime

FROM gcr.io/distroless/java-base-debian13:nonroot
WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /workspace/app/build/install/app/lib/ /app/lib/
COPY --chown=nonroot:nonroot LICENSE /app/LICENSE
COPY --from=builder --chown=nonroot:nonroot /workspace/app/build/reports/dependency-license/ /app/THIRD_PARTY_LICENSES/
COPY --from=builder --chown=nonroot:nonroot /workspace/runtime/ /app/
COPY --from=runtime-builder /runtime /runtime
ENTRYPOINT ["/runtime/bin/java", "-Dfile.encoding=UTF-8", "-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8", "-cp", "/app/lib/*", "io.github.anvilsaba.mcguildlink.app.AppKt"]
