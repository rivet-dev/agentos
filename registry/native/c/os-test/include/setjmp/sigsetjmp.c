#include <setjmp.h>
#ifndef sigsetjmp
int (*foo)(sigjmp_buf, int) = sigsetjmp;
#endif
int main(void) { return 0; }
