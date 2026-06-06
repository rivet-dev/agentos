#include <ctype.h>
#ifdef isdigit
#undef isdigit
#endif
int (*foo)(int) = isdigit;
int main(void) { return 0; }
