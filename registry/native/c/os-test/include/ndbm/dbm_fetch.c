/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_fetch
#undef dbm_fetch
#endif
datum (*foo)(DBM *, datum) = dbm_fetch;
int main(void) { return 0; }
