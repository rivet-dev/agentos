#include <stdlib.h>
#ifdef quick_exit
#undef quick_exit
#endif
 void (*foo)(int) = quick_exit;
int main(void) { return 0; }
