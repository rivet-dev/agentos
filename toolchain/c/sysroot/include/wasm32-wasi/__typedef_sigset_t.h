#ifndef __wasilibc___typedef_sigset_t_h
#define __wasilibc___typedef_sigset_t_h

/* Keep this in sync with musl so sigaction() can preserve real signal masks. */
typedef struct __sigset_t { unsigned long __bits[128/sizeof(long)]; } sigset_t;

#endif
