#include <langinfo.h>
#ifdef nl_langinfo_l
#undef nl_langinfo_l
#endif
char *(*foo)(nl_item, locale_t) = nl_langinfo_l;
int main(void) { return 0; }
