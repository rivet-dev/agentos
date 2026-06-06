#include <langinfo.h>
#ifdef nl_langinfo
#undef nl_langinfo
#endif
char *(*foo)(nl_item) = nl_langinfo;
int main(void) { return 0; }
