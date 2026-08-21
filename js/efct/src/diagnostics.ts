export type DiagnosticSeverity = "Error" | "Warning";

export interface EfctDiagnostic {
  readonly code: string;
  readonly severity: DiagnosticSeverity;
  readonly filename: string;
  readonly function: string | null;
  readonly message: string;
  readonly trace: readonly string[];
  readonly effect_trace?: readonly unknown[];
  readonly suggestion: string | null;
  readonly span: unknown | null;
}
