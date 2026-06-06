#include <stdlib.h>
#ifdef srand
#undef srand
#endif
void (*foo)(unsigned) = srand;
int main(void) { return 0; }
