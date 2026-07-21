import { useCallback, useEffect, useState } from "react";

export function useGatewayResource(loader, dependencies = []) {
  const [state, setState] = useState({ loading: Boolean(loader), value: null, error: null });

  const refresh = useCallback(async () => {
    if (!loader) {
      setState({ loading: false, value: null, error: null });
      return null;
    }
    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const value = await loader();
      setState({ loading: false, value, error: null });
      return value;
    } catch (error) {
      setState({ loading: false, value: null, error });
      return null;
    }
  }, [loader]);

  useEffect(() => {
    void refresh();
  }, [refresh, ...dependencies]);

  return { ...state, refresh };
}
