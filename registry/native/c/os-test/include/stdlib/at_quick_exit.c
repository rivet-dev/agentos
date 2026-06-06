#include <stdlib.h>
#ifdef at_quick_exit
#undef at_quick_exit
#endif
int (*foo)(void (*)(void)) = at_quick_exit;
int main(void) { return 0; }
