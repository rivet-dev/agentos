#include <stdlib.h>
#ifdef exit
#undef exit
#endif
 void (*foo)(int) = exit;
int main(void) { return 0; }
