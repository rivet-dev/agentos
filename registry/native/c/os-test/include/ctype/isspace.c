#include <ctype.h>
#ifdef isspace
#undef isspace
#endif
int (*foo)(int) = isspace;
int main(void) { return 0; }
