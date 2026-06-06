#include <ctype.h>
#ifdef isalnum
#undef isalnum
#endif
int (*foo)(int) = isalnum;
int main(void) { return 0; }
