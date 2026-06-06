#include <signal.h>
#ifdef str2sig
#undef str2sig
#endif
int (*foo)(const char *restrict, int *restrict) = str2sig;
int main(void) { return 0; }
