#include <ctype.h>
#ifdef isblank
#undef isblank
#endif
int (*foo)(int) = isblank;
int main(void) { return 0; }
