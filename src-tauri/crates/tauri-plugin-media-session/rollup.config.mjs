import typescript from '@rollup/plugin-typescript';

export default {
  input: 'guest-js/index.ts',
  output: {
    file: 'dist-js/index.mjs',
    format: 'esm',
  },
  plugins: [
    typescript({
      declaration: true,
      declarationDir: './dist-js',
    }),
  ],
  external: [/^@tauri-apps\/api/],
};
