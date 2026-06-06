#include <stdlib.h>
#ifdef setenv
#undef setenv
#endif
int (*foo)(const char *, const char *, int) = setenv;
int main(void) { return 0; }
