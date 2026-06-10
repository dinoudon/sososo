// Enforce Conventional Commits (feat / fix / refactor / chore / docs / test / ci …)
// in CI via wagoid/commitlint-github-action. Run locally with:
//   bunx commitlint --from=HEAD~1
//
// The conventional preset caps body/footer lines at 100 chars, which hard-fails
// commits whose bodies are detailed prose or contain long URLs/paths (e.g. the
// license/docs commits). We keep the structural rules (type, scope, subject) but
// turn off the line-length caps so descriptive bodies and footers don't break CI.
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'body-max-line-length': [0, 'always', Infinity],
    'footer-max-line-length': [0, 'always', Infinity],
  },
};
