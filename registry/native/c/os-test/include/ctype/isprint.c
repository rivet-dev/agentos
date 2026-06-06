#include <ctype.h>
#ifdef isprint
#undef isprint
#endif
int (*foo)(int) = isprint;
int main(void) { return 0; }
