# HyperStack bench: Redmine's default data, the REST API on, Markdown
# (CommonMark), the admin password from HS_ADMIN_PASSWORD, project "demo" with
# its trackers and three issues. Safe to re-run. Prints the admin's API key.
#
#   docker compose exec -T -e HS_ADMIN_PASSWORD="$REDMINE_ADMIN_PASSWORD" redmine \
#     bundle exec rails runner /hs-seed/seed.rb

Redmine::DefaultData::Loader.load('en') if Redmine::DefaultData::Loader.no_data?

Setting.rest_api_enabled = '1'
Setting.text_formatting = 'common_mark'
Setting.login_required = '0'

admin = User.find_by_login('admin') || raise('no admin user')
password = ENV.fetch('HS_ADMIN_PASSWORD') # REDMINE_ADMIN_PASSWORD from bench/.env
unless admin.check_password?(password)
  admin.password = password
  admin.password_confirmation = password
end
admin.must_change_passwd = false
admin.save!(validate: false)
User.current = admin

project = Project.find_by_identifier('demo') ||
          Project.create!(name: 'Demo', identifier: 'demo', is_public: true)
project.trackers = Tracker.sorted.to_a
project.enabled_module_names = Redmine::AccessControl.available_project_modules.map(&:to_s)
project.save!

manager = Role.find_by_name('Manager')
if manager && !Member.exists?(project_id: project.id, user_id: admin.id)
  Member.create!(project: project, principal: admin, role_ids: [manager.id])
end

def status(name) = IssueStatus.find_by_name(name) || IssueStatus.first
def priority(name) = IssuePriority.find_by_name(name) || IssuePriority.default

bug = Tracker.find_by_name('Bug') || Tracker.first
[
  { subject: 'Seed issue', description: 'Some **bold** text',
    status: 'New', priority: 'Normal' },
  { subject: 'Disk full on db-01', description: "The **disk** is full.\r\n\r\nRotate logs daily.",
    status: 'In Progress', priority: 'High', assigned_to: admin, due_date: Date.new(2027, 1, 15) },
  { subject: 'Backups slow on Sunday', description: 'Takes 3 hours.',
    status: 'New', priority: 'Normal' }
].each do |attrs|
  next if Issue.exists?(project_id: project.id, subject: attrs[:subject])

  Issue.create!(
    project: project, tracker: bug, author: admin,
    subject: attrs[:subject], description: attrs[:description],
    status: status(attrs[:status]), priority: priority(attrs[:priority]),
    assigned_to: attrs[:assigned_to], start_date: Date.today, due_date: attrs[:due_date]
  )
end

puts "REDMINE_API_KEY=#{admin.api_key}"
