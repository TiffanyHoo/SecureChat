/**
 * @type {import('@commitlint/types').UserConfig}
 */
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // 类型限定为项目实际使用的类型
    'type-enum': [
      2,
      'always',
      ['feat', 'fix', 'docs', 'style', 'refactor', 'perf', 'test', 'build', 'ci', 'chore', 'revert'],
    ],
    // 中文描述一行字符数较多，放宽 header 长度限制（默认 100）
    'header-max-length': [2, 'always', 200],
    // 中文 subject 中冒号后空格校验由 subject-empty 等内置规则覆盖
    'subject-empty': [2, 'never'],
    'type-empty': [2, 'never'],
  },
};
