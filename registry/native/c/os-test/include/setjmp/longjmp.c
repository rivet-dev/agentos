#include <setjmp.h>
#ifdef longjmp
#undef longjmp
#endif
 void (*foo)(jmp_buf, int) = longjmp;
int main(void) { return 0; }
