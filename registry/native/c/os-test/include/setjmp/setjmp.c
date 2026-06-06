#include <setjmp.h>
#ifndef setjmp
int (*foo)(jmp_buf) = setjmp;
#endif
int main(void) { return 0; }
