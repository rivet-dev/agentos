/*[XSI|SIO]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/mman.h>
int const foo = MS_ASYNC;
int main(void) { return 0; }
