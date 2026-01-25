export type UtilConfig = {
  enabled: boolean;
};

export function util(cfg: UtilConfig) {
  return cfg.enabled;
}
