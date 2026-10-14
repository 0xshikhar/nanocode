export type RetryAfterAdvice = {
  retry_after?: number;
  retry_after_deadline_ms?: number;
};

export declare function retryAfterAdvice(value: unknown, receivedAt?: number, receivedMonotonic?: number): RetryAfterAdvice;

export declare function retryAfterRemaining(advice: RetryAfterAdvice, now?: number, currentMonotonic?: number): number | undefined;

export declare function attachRetryAfterAdvice<T extends object>(target: T, advice: RetryAfterAdvice): T & RetryAfterAdvice;
