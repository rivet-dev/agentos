#include <ctype.h>
#ifdef islower
#undef islower
#endif
int (*foo)(int) = islower;
int main(void) { return 0; }
