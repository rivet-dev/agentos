#include <ctype.h>
#ifdef isgraph
#undef isgraph
#endif
int (*foo)(int) = isgraph;
int main(void) { return 0; }
