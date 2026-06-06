#include <stdlib.h>
#ifdef abs
#undef abs
#endif
int (*foo)(int) = abs;
int main(void) { return 0; }
