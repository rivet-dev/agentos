#include <setjmp.h>
#ifdef siglongjmp
#undef siglongjmp
#endif
 void (*foo)(sigjmp_buf, int) = siglongjmp;
int main(void) { return 0; }
