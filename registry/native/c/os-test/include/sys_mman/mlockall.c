/*[ML]*/
#include <sys/mman.h>
#ifdef mlockall
#undef mlockall
#endif
int (*foo)(int) = mlockall;
int main(void) { return 0; }
