#32 7.847 
#32 8.111 DEPRECATION WARNING [legacy-js-api]: The legacy JS API is deprecated and will be removed in Dart Sass 2.0.0.
#32 8.111 
#32 8.111 More info: https://sass-lang.com/d/legacy-js-api
#32 8.111 
#32 8.721 ✓ 426 modules transformed.
#32 8.867 rendering chunks...
#32 8.887 computing gzip size...
#32 8.897 dist/index.html                   0.47 kB │ gzip:  0.30 kB
#32 8.897 dist/assets/index-DbwWKqIN.css   44.54 kB │ gzip:  6.30 kB
#32 8.897 dist/assets/index-BbdYskam.js    93.70 kB │ gzip: 25.55 kB
#32 8.897 dist/assets/vendor-B83g54At.js  100.40 kB │ gzip: 39.24 kB
#32 8.898 ✓ built in 3.82s
#32 8.974 npm notice
#32 8.974 npm notice New major version of npm available! 11.16.0 -> 12.0.2
#32 8.974 npm notice Changelog: https://github.com/npm/cli/releases/tag/v12.0.2
#32 8.974 npm notice To update run: npm install -g npm@12.0.2
#32 8.974 npm notice
#32 DONE 9.3s

#33 [stage-4 5/5] COPY --from=node-builder /app/frontend/dist ./frontend/dist/
#33 DONE 0.2s

#34 exporting to image
#34 exporting layers
#34 exporting layers 1.3s done
#34 exporting manifest sha256:9ea2d80aff7880ae1f373885c73a0d90557d44b4a9db449842e0fb929cea74f1 0.0s done
#34 exporting config sha256:4f087eaad531415caf70c6811329062d478dd507d82f82a4e90cca4d02c62a09 0.0s done
#34 exporting attestation manifest sha256:ff4f8040ad20ec93f9a4e578d30c0709748a722cca3dc219ece621a86be3e865 0.1s done
#34 exporting manifest list sha256:7a5867db9ce6dd596681c3fdf6a4a7ff7cf717283650424514a9b8029941054c
#34 exporting manifest list sha256:7a5867db9ce6dd596681c3fdf6a4a7ff7cf717283650424514a9b8029941054c 0.0s done
#34 naming to docker.io/library/t2t-full-test:latest done
#34 unpacking to docker.io/library/t2t-full-test:latest
#34 unpacking to docker.io/library/t2t-full-test:latest 0.4s done
#34 DONE 1.9s

 [33m1 warning found (use docker --debug to expand):
[0m - SecretsUsedInArgOrEnv: Do not use ARG or ENV instructions for sensitive data (ARG "BUILD_BUGSINK_AUTH_TOKEN") (line 77)

View build details: docker-desktop://dashboard/build/default/default/lvq97d57wx8vqgek1uu85pq6n

real	0m42,160s
user	0m0,436s
sys	0m0,256s
