POSIX_HTML=../susv5-html
APIS=$(basename $(basename $(notdir $(wildcard $(POSIX_HTML)/basedefs/*.h.html))))
API_FILES=$(addprefix ../include/,$(addsuffix .api,$(APIS)))

.PHONY: api
api: $(POSIX_HTML)
	$(MAKE) api-recursive

.PHONY: api-recursive
api-recursive: $(API_FILES)

../susv5.tgz:
	wget https://pubs.opengroup.org/onlinepubs/9799919799.2024edition/download/susv5.tgz -O ../susv5.tgz

../susv5-html: ../susv5.tgz
	echo "11c7ee3e98febe3d58d03a02885d0fab9a3564d06a9b02742d9d138ae341a957  ../susv5.tgz" | sha256sum -c
	tar -xzf ../susv5.tgz -C ..
	touch -r ../susv5.tgz ../susv5-html

../include:
	mkdir -p $@

$(API_FILES): posix-parse $(POSIX_HTML) | ../include

../include/%.api: $(POSIX_HTML)/basedefs/%.h.html
	./posix-parse $< $(POSIX_HTML)/functions/V2_chap02.html > $@

.PHONY: clean
clean:
	rm -f posix-parse

.PHONY: clean-api
clean-api:
	rm -f ../include/*.api
	for api in $(APIS); do rm -rf ../include/$$api; done

.PHONY: clean-posix-tarball
clean-posix-tarball:
	rm -rf ../susv5.tgz

.PHONY: clean-posix-html
clean-posix-html:
	rm -rf ../susv5-html

.PHONY: clean-everything
clean-everything: clean clean-api clean-posix-tarball clean-posix-html
